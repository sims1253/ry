use super::*;
use crate::infer::json_rtype_to_rtype;

impl Checker {
    /// Resolve only signatures that declare checker schema semantics. Unlike
    /// ordinary call resolution, a same-named base function without an effect
    /// does not mask an attached package's declarative verb.
    pub(crate) fn resolve_schema_sig(&self, name: &str) -> Option<FunctionSig> {
        if let Some((pkg_raw, fun)) = name.rsplit_once("::") {
            let pkg = pkg_raw.trim_end_matches(':');
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
        if let Some(package) = self.imported_from.get(name) {
            if let Some(sig) = self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name))
                .filter(|sig| has_schema_semantics(sig))
            {
                return Some(sig.clone());
            }
        }
        if let Some(sig) = self
            .typeshed
            .functions
            .get(name)
            .filter(|sig| has_schema_semantics(sig))
        {
            return Some(sig.clone());
        }
        for package in self.available_package_names() {
            let attached = self.loaded.contains(package)
                || (self.loaded.contains("tidyverse") && matches!(package, "dplyr" | "tidyr"));
            if !attached {
                continue;
            }
            if let Some(sig) = self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name))
                .filter(|sig| has_schema_semantics(sig))
            {
                return Some(sig.clone());
            }
        }
        None
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
        for package in self.available_package_names() {
            if !self.bare_loaded.contains(package) {
                continue;
            }
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
        if let Some((pkg_raw, value)) = name.rsplit_once("::") {
            let package = pkg_raw.trim_end_matches(':');
            if matches!(
                package,
                "base" | "stats" | "utils" | "graphics" | "grDevices" | "methods" | "datasets"
            ) && let Some(value_type) = self.typeshed.datasets.get(value)
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
        for package in self.available_package_names() {
            if self.bare_loaded.contains(package)
                && let Some(value_type) = self
                    .package_typeshed(package)
                    .and_then(|typeshed| typeshed.datasets.get(name))
            {
                return Some(json_rtype_to_rtype(value_type));
            }
        }
        None
    }

    pub(crate) fn resolve_typeshed_sig(&self, name: &str) -> Option<FunctionSig> {
        // Qualified call: explicit package reference.
        if let Some((pkg_raw, fun)) = name.rsplit_once("::") {
            // `pkg:::fun` splits as ("pkg:", "fun"); trim the trailing
            // colon to recover the package name.
            let pkg = pkg_raw.trim_end_matches(':');
            // R's standard packages share our embedded base database. This
            // is an explicit package-to-database mapping, not a fallback to
            // a similarly named export from another package.
            if matches!(
                pkg,
                "base" | "stats" | "utils" | "graphics" | "grDevices" | "methods" | "datasets"
            ) && let Some(sig) = self.typeshed.functions.get(fun)
            {
                return Some(sig.clone());
            }
            if let Some(t) = self.package_typeshed(pkg) {
                if let Some(sig) = t.functions.get(fun) {
                    return Some(sig.clone());
                }
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
        if let Some(package) = self.imported_from.get(name) {
            if let Some(signature) = self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name).cloned())
            {
                return Some(signature);
            }
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
        for pkg in self.available_package_names() {
            if !self.bare_loaded.contains(pkg) {
                continue;
            }
            if let Some(t) = self.package_typeshed(pkg) {
                if let Some(sig) = t.functions.get(name) {
                    return Some(sig.clone());
                }
            }
        }
        None
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
        if let Some((pkg_raw, fun)) = name.rsplit_once("::") {
            let pkg = pkg_raw.trim_end_matches(':');
            if let Some(t) = self.package_typeshed(pkg) {
                if t.functions.contains_key(fun) {
                    return true;
                }
            }
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
        for pkg in self.available_package_names() {
            if !self.bare_loaded.contains(pkg) {
                continue;
            }
            if let Some(t) = self.package_typeshed(pkg) {
                if t.functions.contains_key(name) {
                    return true;
                }
            }
        }
        self.fn_table.fns.contains_key(name) || self.fn_table.callable_vars.contains(name)
    }

    pub(crate) fn resolves_user_s3_dispatch(&self, generic: &str, first: &RType) -> bool {
        self.user_s3_dispatch_return(generic, first).is_some()
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

    // Apply a `SeverityFilter` to the diagnostics collected so far,
    // mutating severities (or dropping suppressed ones) in place.
    pub fn apply_filter(&mut self, filter: &SeverityFilter) {
        apply_filter_to_diagnostics(&mut self.diagnostics, filter);
    }

    pub(crate) fn emit(
        &mut self,
        severity: Severity,
        span: Span,
        code: &'static str,
        msg: impl Into<String>,
    ) {
        self.emit_optional_fix(severity, span, code, msg, None);
    }

    pub(crate) fn emit_with_fix(
        &mut self,
        severity: Severity,
        span: Span,
        code: &'static str,
        msg: impl Into<String>,
        fix: Fix,
    ) {
        self.emit_optional_fix(severity, span, code, msg, Some(fix));
    }

    fn emit_optional_fix(
        &mut self,
        severity: Severity,
        span: Span,
        code: &'static str,
        msg: impl Into<String>,
        fix: Option<Fix>,
    ) {
        if self.discarding {
            // Pass 2 (fixpoint) and closure-signature building run the
            // single inference engine in "discarding" mode: types are
            // computed but no diagnostics are recorded. This keeps pass 2
            // from double-emitting (diagnostics are produced in pass 3
            // against the refined FnTable).
            return;
        }
        let mut diagnostic = Diagnostic::new(severity, span, &self.path, code, msg);
        diagnostic.fix = fix;
        self.diagnostics.push(diagnostic);
    }

    /// Slice the exact parser input. Fix producers use AST spans and this
    /// source seam; they never recover replacement text from a diagnostic
    /// message or attempt to pretty-print an AST.
    pub(crate) fn source_text(&self, span: Span) -> Option<&str> {
        self.source.get(span.start..span.end)
    }

    pub(crate) fn source_span(&self, start: usize, end: usize) -> Option<Span> {
        if start > end || !self.source.is_char_boundary(start) || !self.source.is_char_boundary(end)
        {
            return None;
        }
        let prefix = self.source.get(..start)?;
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
        let line_start = prefix.rfind('\n').map_or(0, |offset| offset + 1);
        Some(Span::new(start, end, line, start - line_start))
    }

    pub(crate) fn fix(&self, span: Span, replacement: impl Into<String>) -> Option<Fix> {
        self.source_text(span)?;
        Some(Fix {
            span,
            replacement: replacement.into(),
        })
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

    // Pass 1: walk top-level (and only top-level) statements, collecting
    // function definitions of the form `name <- function(...) body` into
    // the FnTable. Nested function definitions are recorded only if they
    // are themselves bound to a name at their enclosing scope; this is
    // sufficient for v2 since R-style nested defs typically close over
    // locals and are tricky to type without proper closure analysis.
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
