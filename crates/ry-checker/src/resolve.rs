//! Typeshed and package signature/value resolution, plus the checker's
//! diagnostic emit helpers.

use super::*;
use crate::infer::json_rtype_to_rtype;

/// R's standard packages, which share ry's embedded base stub database:
/// a qualified lookup in any of them resolves against `typeshed` itself.
const BASE_DATABASE_PACKAGES: &[&str] = &[
    "base",
    "stats",
    "utils",
    "graphics",
    "grDevices",
    "methods",
    "datasets",
];

/// Which attachment set gates the attached-package rung of a
/// typeshed-resolution ladder. The checker keeps two sets with different
/// granularity (`Checker::loaded` is a project-wide union used for dplyr
/// NSE gating; `Checker::bare_loaded` is this file's R search path), so
/// every ladder must declare its gate explicitly rather than silently
/// sharing one lookup (issue #166).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachedGate {
    /// `bare_loaded`: this file's own search path. The gate for ordinary
    /// bare-name resolution -- signatures, values, predicates, and
    /// `has_function_anywhere`.
    Bare,
    /// `loaded` (the project-wide union) plus the tidyverse expansion:
    /// `library(tidyverse)` makes dplyr and tidyr's declarative verbs
    /// resolvable everywhere, exactly like the dplyr NSE gating they
    /// share inference with.
    SchemaNse,
}

impl Checker {
    /// The attached packages a resolution ladder may consult, in the
    /// deterministic priority order of [`Self::available_package_names`],
    /// filtered by `gate`'s attachment rule. This is the one shared
    /// form of the "walk candidate packages, keep the ones actually
    /// attached" rung that every ladder below used to hand-roll.
    pub(crate) fn candidate_packages(&self, gate: AttachedGate) -> impl Iterator<Item = &str> + '_ {
        self.available_package_names().filter(move |package| match gate {
            AttachedGate::Bare => self.bare_loaded.contains(*package),
            AttachedGate::SchemaNse => {
                self.loaded.contains(*package)
                    || (self.loaded.contains("tidyverse") && matches!(*package, "dplyr" | "tidyr"))
            }
        })
    }

    /// Resolve only signatures that declare checker schema semantics. Unlike
    /// ordinary call resolution, a same-named base function without an effect
    /// does not mask an attached package's declarative verb.
    pub(crate) fn resolve_schema_sig(&self, name: &str) -> Option<FunctionSig> {
        if let Some((pkg, fun)) = split_qualified(name) {
            if let Some(signature) = self
                .package_typeshed(pkg)
                .and_then(|typeshed| typeshed.functions.get(fun))
                .filter(|sig| has_schema_semantics(sig))
            {
                return Some(signature.clone());
            }
            return self
                .typeshed
                .functions
                .get(fun)
                .filter(|sig| has_schema_semantics(sig))
                .cloned();
        }
        if let Some(package) = self.imported_from.get(name)
            && let Some(sig) = self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name))
                .filter(|sig| has_schema_semantics(sig))
        {
            return Some(sig.clone());
        }
        if let Some(sig) = self
            .typeshed
            .functions
            .get(name)
            .filter(|sig| has_schema_semantics(sig))
        {
            return Some(sig.clone());
        }
        self.candidate_packages(AttachedGate::SchemaNse)
            .find_map(|package| {
                self.package_typeshed(package)
                    .and_then(|typeshed| typeshed.functions.get(name))
                    .filter(|sig| has_schema_semantics(sig))
                    .cloned()
            })
    }

    /// Resolve a predicate declaration with exact callee provenance. Bare
    /// package helpers are accepted only when their predicate declaration is
    /// unambiguous across the candidate packages.
    pub(crate) fn resolve_predicate_sig(&self, name: &str) -> Option<FunctionSig> {
        if name.contains("::") {
            return self
                .resolve_typeshed_sig(name)
                .filter(|signature| signature.predicate.is_some());
        }

        // An importFrom binding establishes one exact package origin. Do not
        // scan it again below: ambiguity is about distinct origins, not the
        // number of resolution paths that reach the same declaration.
        if let Some(package) = self.imported_from.get(name) {
            return self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name))
                .filter(|signature| signature.predicate.is_some())
                .cloned();
        }

        let mut candidates = Vec::new();
        if let Some(signature) = self
            .typeshed
            .functions
            .get(name)
            .filter(|signature| signature.predicate.is_some())
            .cloned()
        {
            candidates.push(signature);
        }
        for package in self.candidate_packages(AttachedGate::Bare) {
            if let Some(signature) = self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name))
                .filter(|signature| signature.predicate.is_some())
                .cloned()
            {
                // Each package appears once in available_package_names(), so
                // an attached package cannot be counted both as the ordinary
                // resolution result and as a schema candidate.
                candidates.push(signature);
            }
        }
        (candidates.len() == 1).then(|| candidates.remove(0))
    }

    /// Resolve a typed package value under the same provenance rules as
    /// function signatures. The legacy `datasets` map stores typed exported
    /// values as well as package datasets; unlike `functions`, these names are
    /// never callable.
    pub(crate) fn resolve_typeshed_value(&self, name: &str) -> Option<RType> {
        if let Some((package, value)) = split_qualified(name) {
            if BASE_DATABASE_PACKAGES.contains(&package)
                && let Some(value_type) = self.typeshed.datasets.get(value)
            {
                return Some(json_rtype_to_rtype(value_type));
            }
            return self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.datasets.get(value))
                .map(json_rtype_to_rtype);
        }
        if let Some(package) = self.imported_from.get(name)
            && let Some(value_type) = self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.datasets.get(name))
        {
            return Some(json_rtype_to_rtype(value_type));
        }
        if let Some(value_type) = self.typeshed.datasets.get(name) {
            return Some(json_rtype_to_rtype(value_type));
        }
        self.candidate_packages(AttachedGate::Bare).find_map(|package| {
            self.package_typeshed(package)
                .and_then(|typeshed| typeshed.datasets.get(name))
                .map(json_rtype_to_rtype)
        })
    }

    /// Resolve a function signature by name, consulting (in order):
    ///   1. a `pkg::fun` / `pkg:::fun` qualified name -- looked up in
    ///      `package_typeshed(pkg)` directly, bypassing base and loaded
    ///      packages (a qualified call is an explicit reference);
    ///   2. an unqualified name with a recorded `importFrom` binding --
    ///      resolved against that package's typeshed (exact provenance);
    ///   3. the base typeshed (`self.typeshed`);
    ///   4. each loaded package that ships signatures, in a fixed
    ///      deterministic priority order approximating R's search path.
    ///
    /// Returns the signature; `None` when no package knows the name.
    pub(crate) fn resolve_typeshed_sig(&self, name: &str) -> Option<FunctionSig> {
        // Qualified call: explicit package reference.
        if let Some((pkg, fun)) = split_qualified(name) {
            // R's standard packages share our embedded base database. This
            // is an explicit package-to-database mapping, not a fallback to
            // a similarly named export from another package.
            if BASE_DATABASE_PACKAGES.contains(&pkg)
                && let Some(sig) = self.typeshed.functions.get(fun)
            {
                return Some(sig.clone());
            }
            if let Some(sig) = self
                .package_typeshed(pkg)
                .and_then(|typeshed| typeshed.functions.get(fun))
            {
                return Some(sig.clone());
            }
            // A qualified callee has exact provenance. In particular, do
            // not borrow a same-named base or attached-package signature:
            // `other::f()` is not evidence that `base::f()` was called.
            return None;
        }
        // Unqualified: base typeshed, then loaded packages (fixed
        // priority order; see the comment on masking below).
        // An importFrom binding carries exact provenance without attaching
        // unrelated exports from that package.
        if let Some(package) = self.imported_from.get(name)
            && let Some(signature) = self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name).cloned())
        {
            return Some(signature);
        }
        if let Some(sig) = self.typeshed.functions.get(name) {
            return Some(sig.clone());
        }
        // Loaded packages. R's actual masking depends on search-path
        // position; we approximate with a fixed priority order over the
        // packages that ship signatures (most function names are
        // disjoint across these packages, so masking rarely bites).
        // `loaded` is a HashSet (unordered) so we walk a deterministic
        // known-packages list and check membership.
        self.candidate_packages(AttachedGate::Bare).find_map(|pkg| {
            self.package_typeshed(pkg)
                .and_then(|typeshed| typeshed.functions.get(name))
                .cloned()
        })
    }

    /// Inherit declarative NSE metadata when a source package defines an S3
    /// method without a static NAMESPACE import or registration. Packages such
    /// as dtplyr install several methods dynamically during `.onLoad()`, but a
    /// `<generic>.<class>` definition is still enough to connect the method to
    /// a unique shipped generic signature.
    pub(crate) fn resolve_user_s3_inherited_sig(&self, generic: &str) -> Option<FunctionSig> {
        let method_prefix = format!("{generic}.");
        let has_method = self
            .fn_table
            .fns
            .keys()
            .any(|name| name.starts_with(&method_prefix))
            || self
                .fn_table
                .s3_methods
                .keys()
                .any(|(registered_generic, _)| registered_generic == generic)
            || self
                .external_s3_methods
                .iter()
                .any(|(registered_generic, _)| registered_generic == generic);
        if !has_method {
            return None;
        }

        self.available_package_names()
            .into_iter()
            .find_map(|package| {
                self.package_typeshed(package)
                    .and_then(|typeshed| typeshed.functions.get(generic))
                    .filter(|signature| !signature.eval.is_empty())
                    .cloned()
            })
    }

    // Whether any package (base, loaded, or explicitly qualified)
    // provides a function named `name`. Used by the RY070 path to
    // implement R's function/value namespace separation (a non-function
    // binding is skipped at a call site if a same-named function exists
    // somewhere). Mirrors [`resolve_typeshed_sig`] plus the FnTable.
    pub(crate) fn has_function_anywhere(&self, name: &str) -> bool {
        // Qualified: check the named package.
        if let Some((pkg, fun)) = split_qualified(name)
            && let Some(t) = self.package_typeshed(pkg)
            && t.functions.contains_key(fun)
        {
            return true;
        }
        if self.typeshed.functions.contains_key(name) {
            return true;
        }
        if self
            .typeshed
            .globals
            .ambient_functions
            .iter()
            .any(|function| function == name)
        {
            return true;
        }
        // NAMESPACE imports and S3 registrations are opaque value bindings,
        // but in call position they are also proof that a function candidate
        // exists outside the local value namespace.
        if self.external_bindings.contains(name) {
            return true;
        }
        // Loaded packages (fixed priority order; see resolve_typeshed_sig).
        if self
            .candidate_packages(AttachedGate::Bare)
            .any(|pkg| self.package_typeshed(pkg).is_some_and(|t| t.functions.contains_key(name)))
        {
            return true;
        }
        self.fn_table.fns.contains_key(name) || self.fn_table.callable_vars.contains(name)
    }

    pub(crate) fn resolves_user_s3_dispatch(&self, generic: &str, first: &RType) -> bool {
        self.user_s3_dispatch_return(generic, first).is_some()
    }

    /// Lenient variant of [`Self::resolves_to_base`]: same resolution
    /// order minus the search-path guard, because a loaded package rarely
    /// redefines `list` or `length`.
    pub(crate) fn resolves_to_base_lenient(&self, name: &str, scope: &Scope) -> bool {
        self.resolves_to_base_impl(name, scope, false)
    }

    /// Whether the callee `name` resolves to `base::bare_name` at this call
    /// site, given the current lexical scope and project metadata.
    ///
    /// The canonical base-call resolution operation: callers ask this
    /// method instead of duplicating the lookup order.
    ///
    /// Lookup order (first match decides):
    ///
    /// 1. `base::name` or `base:::name` → resolves to base (explicit).
    /// 2. `otherpkg::name` → does not resolve to base.
    /// 3. A lexical parameter binding of `name` → shadowed.
    /// 4. A lexical function binding of `name` → shadowed.
    /// 5. A project `fn_table` definition of `name` → shadowed.
    /// 6. `importFrom(base, name)` → resolves to base.
    /// 7. Any other external binding or `importFrom` source → shadowed.
    /// 8. A non-empty search path (`bare_loaded` or `search_path_unknown`)
    ///    → cannot prove base resolution.
    /// 9. Otherwise the bare name falls through to base.
    pub(crate) fn resolves_to_base(&self, name: &str, scope: &Scope) -> bool {
        self.resolves_to_base_impl(name, scope, true)
    }

    /// Shared body of the two base-resolution predicates. `guard_search_path`
    /// toggles step 8 of the documented lookup order: the strict variant
    /// refuses to conclude base resolution while any package may be
    /// attached, the lenient one allows it.
    fn resolves_to_base_impl(&self, name: &str, scope: &Scope, guard_search_path: bool) -> bool {
        // (a) Explicit base:: qualification.
        if name.rsplit_once("::").is_some() {
            return crate::semantic_lists::is_base_qualified(name);
        }

        // (b) Lexical shadowing: only callable or parameter bindings shadow
        // a base function at a call site. R's call-position lookup would
        // error on a non-function parameter, but the checker conservatively
        // treats parameters as potentially callable. A non-parameter data
        // binding does not shadow — `c <- 1L; c(a = 1)` still calls base::c
        // (mirrors infer_call's existing skip-non-function-binding rule).
        if scope.is_lexical_function(name) {
            return false;
        }
        if let Some(ty) = scope.get(name)
            && (matches!(ty.mode, ry_core::types::Mode::Function) || scope.is_parameter(name))
        {
            return false;
        }

        // (c) fn_table shadowing.
        if self.fn_table.fns.contains_key(name) {
            return false;
        }

        // (d) external_bindings / imported_from.
        // An explicit importFrom(base, name) is authoritative.
        if let Some(pkg) = self.imported_from.get(name) {
            return pkg == "base";
        }
        if self.external_bindings.contains(name) {
            return false;
        }

        // (e) search_path_unknown or bare-loaded packages may shadow.
        if guard_search_path && (scope.search_path_unknown || !self.bare_loaded.is_empty()) {
            return false;
        }

        // The bare name falls through to base.
        true
    }

    pub(crate) fn user_s3_dispatch_return(&self, generic: &str, first: &RType) -> Option<RType> {
        for class in first
            .class
            .names
            .iter()
            .take(first.class.len as usize)
            .flatten()
        {
            if let Some(result) = self
                .fn_table
                .fns
                .get(&format!("{generic}.{class}"))
                .map(|function| self.return_slots.get(function.return_slot))
                .or_else(|| {
                    self.fn_table
                        .s3_methods
                        .get(&(generic.to_string(), class.to_string()))
                        .map(|slot| self.return_slots.get(*slot))
                })
            {
                return Some(result);
            }
        }
        let mut candidates = self
            .external_s3_methods
            .iter()
            .filter(|(registered_generic, _)| registered_generic == generic)
            .filter_map(|(_, class)| {
                self.fn_table
                    .fns
                    .get(&format!("{generic}.{class}"))
                    .map(|function| function.return_slot)
            });
        let slot = candidates.next()?;
        if candidates.any(|candidate| candidate != slot) {
            return None;
        }
        Some(self.return_slots.get(slot))
    }

    pub(crate) fn emit(
        &mut self,
        severity: Severity,
        span: Span,
        code: &'static str,
        msg: impl Into<String>,
    ) {
        if self.discarding {
            // Pass 2 (fixpoint) and closure-signature building run the
            // single inference engine in "discarding" mode: types are
            // computed but no diagnostics are recorded. This keeps pass 2
            // from double-emitting (diagnostics are produced in pass 3
            // against the refined FnTable).
            return;
        }
        let diagnostic = Diagnostic::new(severity, span, &self.path, code, msg);
        self.diagnostics.push(diagnostic);
    }

    /// Slice the exact parser input. Messages that quote source spelling
    /// (e.g. RY102's `"name" = ...` hint) use AST spans and this source
    /// seam; they never recover text from a diagnostic message field or
    /// attempt to pretty-print an AST.
    pub(crate) fn source_text(&self, span: Span) -> Option<&str> {
        self.source.get(span.start..span.end)
    }

    // Surface parse errors collected by `RParser` as `RY000`
    // (syntax-error) diagnostics. Each tree-sitter `ERROR` / `MISSING`
    // node becomes one diagnostic. Always emitted, regardless of the
    // checker's other findings: a broken region of input is the primary
    // signal that the file is malformed.
    pub(crate) fn emit_parse_errors(&mut self, file: &SourceFile) {
        for span in &file.parse_errors {
            self.emit(
                Severity::Error,
                *span,
                "RY000",
                "syntax error: unparseable region (recovered tree may be unreliable)",
            );
        }
    }
}

fn has_schema_semantics(signature: &FunctionSig) -> bool {
    let has_mask_eval = signature
        .eval
        .values()
        .any(|mode| matches!(mode, EvalMode::DataMask | EvalMode::TidySelect));
    has_mask_eval
        || matches!(
            signature.schema_effect,
            Some(SchemaEffect::Join | SchemaEffect::Pivot)
        )
}

/// Split `pkg::fun` / `pkg:::fun` into `(package, member)`. The triple-
/// colon form splits as `("pkg:", "fun")`, so trailing colons are
/// trimmed to recover the package name.
fn split_qualified(name: &str) -> Option<(&str, &str)> {
    let (pkg_raw, member) = name.rsplit_once("::")?;
    Some((pkg_raw.trim_end_matches(':'), member))
}
