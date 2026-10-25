//! Typeshed and package signature/value resolution, plus the checker's
//! diagnostic emit helpers.

use super::*;
use crate::infer::{dispatch_result, json_rtype_to_rtype};

/// R's standard packages, which share ry's embedded base stub database:
/// a qualified lookup in any of them resolves against `typeshed` itself.
pub(crate) const BASE_DATABASE_PACKAGES: &[&str] = &[
    "base",
    "stats",
    "utils",
    "graphics",
    "grDevices",
    "methods",
    "datasets",
];

/// Which database a stub lookup found its answer in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaProvider {
    Base,
    Package,
}

/// What a [`Checker::stub_ladder`] walk resolves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StubLookup {
    /// Signatures, values, and predicates.
    Ordinary,
    /// Declarative verbs. Attached packages come from the project-wide
    /// `loaded` union, and a qualified name reads its package stub before
    /// the base database.
    Schema,
}

/// Whether a direct call may use package-specific inference.
pub(crate) enum SpecialCallProvenance {
    Proven,
    Ordinary,
    /// Ambient data-mask or search-path metadata cannot establish identity.
    AmbientUncertainty,
    Unknown,
}

impl Checker {
    /// Spelling aliases and uncertain bindings cannot prove a captured callable.
    /// Known custom contracts keep the ordinary call path instead.
    pub(crate) fn special_call_provenance(
        &self,
        original_name: &str,
        original_callee: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        package: &str,
        scope: &Scope,
    ) -> SpecialCallProvenance {
        self.special_call_provenance_after_effects(
            original_name,
            original_callee,
            semantic_name,
            lookup_name,
            package,
            scope,
            false,
        )
    }

    /// [`Self::special_call_provenance`] where `effects_unknown` means an
    /// earlier call may have rebound the callee or `::`. A visible binding
    /// to another function still decides a bare name.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn special_call_provenance_after_effects(
        &self,
        original_name: &str,
        original_callee: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        package: &str,
        scope: &Scope,
        effects_unknown: bool,
    ) -> SpecialCallProvenance {
        if original_name != semantic_name
            || (matches!(original_callee, Expr::String(_, _)) && semantic_name.contains("::"))
        {
            return SpecialCallProvenance::Unknown;
        }
        if let Some((prefix, _)) = semantic_name.rsplit_once("::") {
            if effects_unknown
                || self.literal_bindings_may_be_shadowed(
                    ["::", "`::`", ":::", "`:::`"],
                    &HashSet::new(),
                    scope,
                )
            {
                return SpecialCallProvenance::Unknown;
            }
            if prefix.trim_end_matches(':') != package {
                // Only an actual package contract can describe another
                // callable with this spelling; avoid the shared standard
                // package database lending it a base signature.
                return if self
                    .package_typeshed(prefix.trim_end_matches(':'))
                    .is_some_and(|typeshed| typeshed.functions.contains_key(lookup_name))
                {
                    SpecialCallProvenance::Ordinary
                } else {
                    SpecialCallProvenance::Unknown
                };
            }
        } else {
            if scope.is_parameter(semantic_name) {
                // An untyped callable formal must not fall back to a base stub.
                return SpecialCallProvenance::Unknown;
            }
            // Known custom functions keep the ordinary call path.
            if self.fn_table.functions.binds(semantic_name)
                || scope
                    .get(semantic_name)
                    .is_some_and(|ty| ty.fn_sig.is_some())
            {
                return SpecialCallProvenance::Ordinary;
            }
            if let Some(imported) = self.imported_from.get(semantic_name)
                && imported != package
            {
                return if self
                    .package_typeshed(imported)
                    .is_some_and(|typeshed| typeshed.functions.contains_key(semantic_name))
                {
                    SpecialCallProvenance::Ordinary
                } else {
                    SpecialCallProvenance::Unknown
                };
            }
            if effects_unknown
                || self.literal_bindings_may_shadow_package(
                    [semantic_name, format!("`{semantic_name}`").as_str()],
                    &HashSet::new(),
                    scope,
                    package,
                )
            {
                return SpecialCallProvenance::Unknown;
            }
            let explicit_import = self
                .imported_from
                .get(semantic_name)
                .is_some_and(|pkg| pkg == package);
            if scope.data_mask_unknown
                || (!explicit_import && self.search_path_may_mask_base(scope))
            {
                return SpecialCallProvenance::AmbientUncertainty;
            }
        }
        if self.user_stubs.contains_key(package) || self.user_stubs.contains_key("base") {
            return SpecialCallProvenance::Ordinary;
        }
        SpecialCallProvenance::Proven
    }

    /// The stub databases that may define `name`, in lookup order, each with
    /// the member to look up there and which kind of database it is:
    ///
    /// 1. `pkg::fun` or `pkg:::fun`: the base database when `pkg` is one of
    ///    [`BASE_DATABASE_PACKAGES`], and `pkg`'s own stub. Nothing else, since
    ///    `other::f()` is no evidence that `base::f()` was called.
    /// 2. A bare name with an `importFrom` binding: that package's stub.
    /// 3. The base database.
    /// 4. The attached packages, in the fixed priority order of
    ///    [`Self::available_package_names`].
    ///
    /// R masks by search-path position. The fixed order approximates it,
    /// since most names are disjoint across the shipped packages.
    fn stub_ladder<'s, 'n>(
        &'s self,
        name: &'n str,
        lookup: StubLookup,
    ) -> impl Iterator<Item = (&'s Typeshed, &'n str, SchemaProvider)> {
        let package = |package: &str| {
            self.package_typeshed(package)
                .map(|typeshed| (typeshed, SchemaProvider::Package))
        };
        let (member, exact, open) = match split_qualified(name) {
            Some((pkg, member)) => {
                let base = BASE_DATABASE_PACKAGES
                    .contains(&pkg)
                    .then_some((self.typeshed.as_ref(), SchemaProvider::Base));
                // Only a user stub named after a standard package tells
                // these orders apart.
                let exact = if lookup == StubLookup::Schema {
                    [package(pkg), base]
                } else {
                    [base, package(pkg)]
                };
                (member, exact, false)
            }
            None => {
                let import = self.imported_from.get(name).and_then(|pkg| package(pkg));
                (name, [import, None], true)
            }
        };
        exact
            .into_iter()
            .flatten()
            .chain(open.then(|| self.open_stubs(lookup)).into_iter().flatten())
            .map(move |(typeshed, provider)| (typeshed, member, provider))
    }

    /// Rungs 3 and 4 of [`Self::stub_ladder`]: the base database, then the
    /// attached packages. Ordinary lookup reads this file's own search path,
    /// `bare_loaded`. Schema lookup reads `loaded`, the project-wide union,
    /// so declarative verbs resolve everywhere, like the dplyr NSE gating
    /// they share inference with.
    fn open_stubs(
        &self,
        lookup: StubLookup,
    ) -> impl Iterator<Item = (&Typeshed, SchemaProvider)> + '_ {
        let attached = if lookup == StubLookup::Schema {
            &self.loaded
        } else {
            &self.bare_loaded
        };
        std::iter::once((self.typeshed.as_ref(), SchemaProvider::Base)).chain(
            self.available_package_names()
                .filter(move |package| attached.contains(*package))
                .filter_map(|package| {
                    self.package_typeshed(package)
                        .map(|typeshed| (typeshed, SchemaProvider::Package))
                }),
        )
    }

    /// Resolve only signatures that declare checker schema semantics. Unlike
    /// ordinary call resolution, a same-named base function without an effect
    /// does not mask an attached package's declarative verb.
    pub(crate) fn resolve_schema_sig(&self, name: &str) -> Option<(FunctionSig, SchemaProvider)> {
        self.stub_ladder(name, StubLookup::Schema)
            .find_map(|(typeshed, member, provider)| {
                typeshed
                    .functions
                    .get(member)
                    .filter(|sig| has_schema_semantics(sig))
                    .map(|sig| (sig.clone(), provider))
            })
    }

    /// Resolve a predicate declaration with exact callee provenance. Bare
    /// package helpers are accepted only when their predicate declaration is
    /// unambiguous across the candidate packages.
    pub(crate) fn resolve_predicate_sig(&self, name: &str) -> Option<FunctionSig> {
        let is_predicate = |signature: &&FunctionSig| signature.predicate.is_some();
        if name.contains("::") {
            return self
                .resolve_typeshed_sig(name)
                .filter(|signature| signature.predicate.is_some());
        }
        // An importFrom binding is one exact origin. Ambiguity counts
        // distinct origins, not the rungs that reach the same declaration.
        if let Some(package) = self.imported_from.get(name) {
            return self
                .package_typeshed(package)
                .and_then(|typeshed| typeshed.functions.get(name))
                .filter(is_predicate)
                .cloned();
        }
        // Each package appears once in available_package_names(), so one
        // declaration cannot count twice.
        let mut predicates = self
            .open_stubs(StubLookup::Ordinary)
            .filter_map(|(typeshed, _)| typeshed.functions.get(name))
            .filter(is_predicate);
        let first = predicates.next()?;
        predicates.next().is_none().then(|| first.clone())
    }

    /// Resolve a typed package value under the same provenance rules as
    /// function signatures. The legacy `datasets` map stores typed exported
    /// values as well as package datasets; unlike `functions`, these names are
    /// never callable.
    pub(crate) fn resolve_typeshed_value(&self, name: &str) -> Option<RType> {
        self.stub_ladder(name, StubLookup::Ordinary)
            .find_map(|(typeshed, member, _)| typeshed.datasets.get(member))
            .map(json_rtype_to_rtype)
    }

    /// Resolve a function signature by name through [`Self::stub_ladder`].
    /// Returns `None` when no stub knows the name.
    pub(crate) fn resolve_typeshed_sig(&self, name: &str) -> Option<FunctionSig> {
        self.resolve_typeshed_sig_ref(name).cloned()
    }

    /// [`Self::resolve_typeshed_sig`] without the clone, for read-only use.
    pub(crate) fn resolve_typeshed_sig_ref<'s>(&'s self, name: &str) -> Option<&'s FunctionSig> {
        self.stub_ladder(name, StubLookup::Ordinary)
            .find_map(|(typeshed, member, _)| typeshed.functions.get(member))
    }

    /// Inherit declarative NSE metadata when a source package defines an S3
    /// method without a static NAMESPACE import or registration. Packages such
    /// as dtplyr install several methods dynamically during `.onLoad()`, but a
    /// `<generic>.<class>` definition is still enough to connect the method to
    /// a unique shipped generic signature.
    pub(crate) fn resolve_user_s3_inherited_sig(&self, generic: &str) -> Option<FunctionSig> {
        let has_method = self.fn_table.functions.has_method_prefix(generic)
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
        self.has_external_function(name)
            || self.fn_table.functions.binds(name)
            || self.fn_table.callable_vars.contains(name)
            || self.fn_table.functions.s4().may_bind_generic(name)
    }

    /// Function candidates outside the current project's execution frame.
    pub(crate) fn has_external_function(&self, name: &str) -> bool {
        // A qualified name reads its package's own stub without the base
        // database mapping. Its full spelling still reaches the open rungs,
        // where base defines `::` and `:::`.
        split_qualified(name).is_some_and(|(package, member)| {
            self.package_typeshed(package)
                .is_some_and(|typeshed| typeshed.functions.contains_key(member))
        }) || self
            .typeshed
            .globals
            .ambient_functions
            .iter()
            .any(|function| function == name)
            // Treat externally supplied names, including NAMESPACE imports,
            // as possible function candidates outside the local value
            // namespace.
            || self.external_bindings.contains(name)
            || self
                .open_stubs(StubLookup::Ordinary)
                .any(|(typeshed, _)| typeshed.functions.contains_key(name))
    }

    pub(crate) fn resolves_user_s3_dispatch(&self, generic: &str, first: &RType) -> bool {
        self.user_s3_dispatch_return(generic, first).is_some()
    }

    /// Check literal binding names without interpreting `::` as qualification.
    /// Escaped spellings stay conservative until the parser decodes them.
    pub(crate) fn literal_bindings_may_be_shadowed<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
        assigned: &HashSet<String>,
        scope: &Scope,
    ) -> bool {
        self.literal_bindings_may_shadow_package(names, assigned, scope, "base")
    }

    pub(crate) fn literal_bindings_may_shadow_package<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
        assigned: &HashSet<String>,
        scope: &Scope,
        expected_package: &str,
    ) -> bool {
        if self.fn_table.has_escaped_binding_names
            || self.escaped_external_bindings
            || scope.bindings.has_escaped_names()
            || assigned.iter().any(|name| name.contains('\\'))
        {
            return true;
        }
        names.into_iter().any(|name| {
            assigned.contains(name)
                || scope.get(name).is_some()
                || self.fn_table.functions.binds(name)
                || self.fn_table.known_vars.contains(name)
                || self
                    .imported_from
                    .get(name)
                    .is_some_and(|package| package != expected_package)
                || (self.external_bindings.contains(name) && !self.imported_from.contains_key(name))
        })
    }

    /// Whether an attached package may supply a bare name ahead of base.
    /// Package `R/` code without whole-package imports looks up base before
    /// the search path, so there only an unknown search path can.
    pub(crate) fn search_path_may_mask_base(&self, scope: &Scope) -> bool {
        scope.search_path_unknown
            || (!self.package_facts.base_before_search_path && !self.bare_loaded.is_empty())
    }

    /// Lenient variant of [`Self::resolves_to_base`]: same resolution
    /// order minus the search-path guard, because a loaded package rarely
    /// redefines `list` or `length`.
    pub(crate) fn resolves_to_base_lenient(&self, name: &str, scope: &Scope) -> bool {
        self.resolves_to_base_impl(name, scope, false)
    }

    /// [`Self::resolves_to_base_lenient`] for a built-in guard callee
    /// (`is.null`, `length`, ...), extended to quoted bindings, which that
    /// check matches only by raw spelling, and to top-level bindings ry
    /// cannot show hold data.
    pub(crate) fn guard_callee_resolves_to_base(&self, name: &str, scope: &Scope) -> bool {
        if name.contains("::") {
            return self.resolves_to_base_lenient(name, scope);
        }
        let quoted = format!("`{name}`");
        [name, quoted.as_str()].into_iter().all(|spelling| {
            self.resolves_to_base_lenient(spelling, scope)
                && !self.top_level_binding_may_be_function(spelling, scope)
        })
    }

    /// Whether a top-level binding of `spelling` may hold a function.
    /// `known_vars` also holds the unquoted name of a `` `length` <- ``
    /// binding. R skips a binding that holds data when it looks up a call
    /// head, so only a value ry knows to be data is safe.
    fn top_level_binding_may_be_function(&self, spelling: &str, scope: &Scope) -> bool {
        use ry_core::types::Mode;
        self.fn_table.known_vars.contains(spelling)
            && (self.fn_table.callable_vars.contains(spelling)
                || !scope.get(spelling).is_some_and(|ty| {
                    matches!(
                        ty.mode,
                        Mode::Logical
                            | Mode::Integer
                            | Mode::Double
                            | Mode::Complex
                            | Mode::Character
                            | Mode::Raw
                            | Mode::List
                            | Mode::Null
                    )
                }))
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
    /// 8. A search path that may mask base (see
    ///    [`Self::search_path_may_mask_base`]) → cannot prove base resolution.
    /// 9. Otherwise the bare name falls through to base.
    pub(crate) fn resolves_to_base(&self, name: &str, scope: &Scope) -> bool {
        self.resolves_to_base_impl(name, scope, true)
    }

    /// Whether a local or project binding of the bare call head `name` may be
    /// the function R calls, ahead of any same-named package function: the
    /// call-head rule ([`Scope::binding_may_be_callee`]) plus nested and
    /// project function definitions, which call-head resolution selects
    /// separately.
    pub(crate) fn call_head_bound_locally(&self, name: &str, scope: &Scope) -> bool {
        scope.is_lexical_function(name)
            || scope.binding_may_be_callee(name)
            || self.fn_table.functions.binds(name)
    }

    /// Shared body of the two base-resolution predicates. `guard_search_path`
    /// toggles step 8 of the documented lookup order: the strict variant
    /// refuses to conclude base resolution while an attached package may
    /// mask base, the lenient one allows it.
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
        if self.fn_table.functions.binds(name) {
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

        // (e) An attached or unknown package may shadow.
        if guard_search_path && self.search_path_may_mask_base(scope) {
            return false;
        }

        // The bare name falls through to base.
        true
    }

    /// Whether `generic` names a project function whose whole body is
    /// `UseMethod("<generic>")`, so it dispatches on its first argument.
    pub(crate) fn is_user_generic(&self, generic: &str) -> bool {
        self.fn_table
            .functions
            .live_named(generic)
            .is_some_and(|function| {
                matches!(&*function.body, [Stmt::Expr(Expr::Call { args, .. })] if args.len() == 1)
                    && usemethod_generic_name(&function.body).as_deref() == Some(generic)
            })
    }

    /// Whether the project binds any `<generic>.<class>` method for a
    /// project generic, which makes the project own its dispatch surface.
    pub(crate) fn has_user_generic_method(&self, generic: &str) -> bool {
        self.is_user_generic(generic)
            && self
                .fn_table
                .functions
                .names()
                .chain(&self.fn_table.known_vars)
                .any(|name| {
                    name.strip_prefix(generic)
                        .and_then(|rest| rest.strip_prefix('.'))
                        .is_some_and(|class| !class.is_empty())
                })
    }

    /// Whether R's dispatcher runs the literal bound to `name` at `span` as an
    /// S3 method, giving it [`crate::infer::insert_s3_dispatch_context`]. Its
    /// name is `generic.class` for a base, operator, or NAMESPACE-registered
    /// generic, or a top-level `evalqOnLoad()` registration names `name`,
    /// whose only binding is this global literal (#348). The proof ignores
    /// writes no name summarizes (`do.call()`, `eval()`), which lubridate
    /// makes elsewhere.
    pub(crate) fn runs_as_s3_method(&self, name: &str, path: &str, span: Span) -> bool {
        let name = semantic_argument_name(name);
        let method_of = |generic: &str| {
            name.strip_prefix(generic)
                .and_then(|rest| rest.strip_prefix('.'))
                .filter(|class| !class.is_empty())
        };
        let table = &self.fn_table;
        let key = (path.to_string(), span.start, span.end);
        split_s3_method_name(name, &self.typeshed.globals).is_some()
            || semantic_lists::OPERATORS
                .iter()
                .chain(&["!", "&", "|", "%*%", "[", "[[", "$", "[<-", "[[<-", "$<-"])
                .any(|generic| method_of(generic).is_some())
            || self
                .external_s3_methods
                .iter()
                .any(|(generic, class)| method_of(generic) == Some(class))
            || (table.registered_s3_methods.contains(name)
                && !["registerS3method", ".S3method", "evalqOnLoad"]
                    .iter()
                    .any(|function| table.known_vars.contains(*function))
                && table.functions.has_single_definition(name)
                && !table.unrecorded_global_literals.contains(name)
                // `` `hel\x70er` `` may rebind `helper` unseen.
                && !table.has_escaped_binding_names
                && table
                    .capture_literal_bindings
                    .get(name)
                    .is_some_and(|literals| literals.len() == 1 && literals.contains(&key)))
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
                .functions
                .live_named(&format!("{generic}.{class}"))
                .map(|function| self.read_return_slot(function.return_slot))
                .or_else(|| {
                    self.fn_table
                        .s3_methods
                        .get(&(generic.to_string(), class.to_string()))
                        .map(|slot| self.read_return_slot(*slot))
                })
            {
                return Some(dispatch_result(first.class.guarded, result));
            }
        }
        // A registration proves that a method exists, not that this receiver
        // selects it. Even a single registered method can coexist with a
        // default method or methods outside the analyzed package.
        self.external_s3_methods
            .iter()
            .any(|(registered_generic, _)| registered_generic == generic)
            .then(RType::unknown)
    }

    pub(crate) fn emit(
        &mut self,
        severity: Severity,
        span: Span,
        code: &'static str,
        msg: impl Into<String>,
    ) {
        self.emit_related(severity, span, code, msg, Vec::new());
    }

    /// [`Checker::emit`] with secondary locations the caller has proven.
    pub(crate) fn emit_related(
        &mut self,
        severity: Severity,
        span: Span,
        code: &'static str,
        msg: impl Into<String>,
        related: Vec<crate::Related>,
    ) {
        if self.discarding || self.in_dead_arm {
            // Pass 2 (fixpoint) and closure-signature building run the
            // single inference engine in "discarding" mode: types are
            // computed but no diagnostics are recorded. This keeps pass 2
            // from double-emitting (diagnostics are produced in pass 3
            // against the refined FnTable). A literal `if`'s untaken arm
            // is walked for its bindings but R never evaluates it.
            return;
        }
        let diagnostic =
            Diagnostic::new(severity, span, &self.path, code, msg).with_related(related);
        self.diagnostics.push(diagnostic);
    }

    /// Slice the exact parser input. Messages that quote source spelling
    /// (e.g. RY102's `"name" = ...` hint) use AST spans and this source
    /// seam; they never recover text from a diagnostic message field or
    /// attempt to pretty-print an AST.
    pub(crate) fn source_text(&self, span: Span) -> Option<&str> {
        self.source.get(span.start..span.end)
    }

    // Surface report-input boundaries (RY120/RY121) as warnings, then
    // parse errors collected by `RParser` as `RY000` (syntax-error)
    // diagnostics. Each tree-sitter `ERROR` / `MISSING`
    // node becomes one diagnostic, and each native-pipe right-hand side
    // that base R's parser rejects (tree-sitter accepts it; see
    // `SourceFile::syntax_violations`) becomes one more. Always emitted,
    // regardless of the checker's other findings: a broken region of
    // input is the primary signal that the file is malformed.
    // Conversely, when any parse error exists, `emit_diagnostics`
    // suppresses the semantic rules' output for the whole file:
    // diagnostics derived from a recovered tree are artifacts of parser
    // repair, and RY000 already covers the file (issue #380). Pipe
    // violations never trigger that suppression: they are only
    // collected from error-free trees, so the file's other diagnostics
    // remain real.
    pub(crate) fn emit_parse_errors(&mut self, file: &SourceFile) {
        for issue in &file.input_issues {
            self.emit(
                Severity::Warning,
                issue.span,
                issue.code,
                issue.message.clone(),
            );
        }
        for span in &file.parse_errors {
            self.emit(
                Severity::Error,
                *span,
                "RY000",
                "syntax error: unparseable region (recovered tree may be unreliable)",
            );
        }
        for violation in &file.syntax_violations {
            self.emit(
                Severity::Error,
                violation.span,
                "RY000",
                violation.message.clone(),
            );
        }
    }

    // Surface encoding problems found while decoding the file from disk
    // as `RY000` (encoding) diagnostics, mirroring R's parser. Returns
    // whether the file was flagged, so `emit_diagnostics` can suppress
    // the semantic rules for it exactly like it does for recovered trees
    // (#380).
    //
    // A leading UTF-8 BOM comes first (#474): the BOM is valid UTF-8
    // (the decoded text keeps it as a leading U+FEFF character), but R's
    // parser still rejects the file with "unexpected input" at 1:1 in
    // every execution context except `parse(keep.source = TRUE)`. Unlike
    // invalid bytes, position is everything: a U+FEFF anywhere else in
    // the file is an ordinary character R accepts, so this is a flag on
    // the file rather than a span to tolerate per-site. Even a
    // comment-only file is rejected (verified against R 4.6.1), so no
    // comment tolerance applies. R reports the BOM before anything else,
    // so it takes precedence over invalid-UTF-8 spans in the same file:
    // one RY000 per file, at 1:1.
    //
    // Invalid UTF-8 (#376) otherwise mirrors `parse()` in R failing with
    // "invalid multibyte character in parser" instead of checking clean.
    // R's lexer scans comments and `%...%` special-operator tokens as
    // raw bytes without multibyte validation, so spans contained in
    // either do not count; a legacy file whose non-ASCII bytes only
    // decorate comments or accented user operators stays clean, exactly
    // like R. One diagnostic per file at the first surviving span: R
    // reports the first invalid multibyte character and stops, and a
    // transcoded file can otherwise produce one span per legacy byte.
    pub(crate) fn emit_encoding_diagnostics(&mut self, file: &SourceFile) -> bool {
        if file.leading_bom {
            // The BOM occupies decoded bytes 0..3 on row 0, column 0.
            self.emit(
                Severity::Error,
                Span::new(0, 3, 0, 0),
                "RY000",
                "unexpected input at 1:1: file starts with a UTF-8 byte order mark (R's parser rejects it)",
            );
            return true;
        }
        let Some(span) = file
            .invalid_utf8
            .iter()
            .find(|span| !tolerated_by_rs_lexer(file, **span))
        else {
            return false;
        };
        self.emit(
            Severity::Error,
            *span,
            "RY000",
            "invalid multibyte character: source file is not valid UTF-8 (R's parser rejects it)",
        );
        true
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
pub(crate) fn split_qualified(name: &str) -> Option<(&str, &str)> {
    let (pkg_raw, member) = name.rsplit_once("::")?;
    Some((pkg_raw.trim_end_matches(':'), member))
}

/// Whether a byte span sits where R's lexer does no multibyte
/// validation: inside a comment or inside a `%...%` special-operator
/// token. gram.y's `SkipComment` and `SpecialValue` scan these as raw
/// bytes, so `parse()` in R accepts files whose only invalid bytes live
/// there; ry agrees to hold the clean-iff-R-parses invariant in both
/// directions.
fn tolerated_by_rs_lexer(file: &SourceFile, span: Span) -> bool {
    inside_comment(file, span) || inside_special_operator(file, span)
}

/// Whether a byte span sits inside an R comment. A comment runs from its
/// `#` to the end of the line, so the span is contained exactly when a
/// lexically collected comment starts at or before it on the same line.
/// Both `Comment::col` and `Span::col` are byte columns over the same
/// decoded source text, so the comparison is direct. Comments are
/// collected sorted by line and at most one comment starts per line, so
/// a binary search finds the only candidate; a dense legacy file can
/// carry one invalid span per non-ASCII byte, and a linear scan per
/// span made that quadratic.
fn inside_comment(file: &SourceFile, span: Span) -> bool {
    let index = file
        .comments
        .partition_point(|comment| comment.line <= span.line);
    file.comments
        .get(index.wrapping_sub(1))
        .is_some_and(|comment| comment.line == span.line && comment.col <= span.col)
}

/// Whether a byte span sits inside a `%...%` special-operator token.
/// The tokens are collected sorted by start offset and never overlap,
/// so the only containment candidate is the last one starting at or
/// before the span.
fn inside_special_operator(file: &SourceFile, span: Span) -> bool {
    let index = file
        .special_operators
        .partition_point(|operator| operator.start <= span.start);
    file.special_operators
        .get(index.wrapping_sub(1))
        .is_some_and(|operator| span.start >= operator.start && span.end <= operator.end)
}
