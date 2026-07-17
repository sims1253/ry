use super::*;

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

    pub(crate) fn resolve_typeshed_sig(&self, name: &str) -> Option<FunctionSig> {
        // Qualified call: explicit package reference.
        if let Some((pkg_raw, fun)) = name.rsplit_once("::") {
            // `pkg:::fun` splits as ("pkg:", "fun"); trim the trailing
            // colon to recover the package name.
            let pkg = pkg_raw.trim_end_matches(':');
            if let Some(t) = self.package_typeshed(pkg) {
                if let Some(sig) = t.functions.get(fun) {
                    return Some(sig.clone());
                }
            }
            // The package is either unknown to ry (no embedded
            // signatures) or doesn't define `fun`. For base/stats/utils
            // (merged into `base.json`) and any other always-attached
            // package, fall back to the BASE typeshed under the STRIPPED
            // name: `stats::rnorm(10)` resolves as base's `rnorm`.
            if let Some(sig) = self.typeshed.functions.get(fun) {
                return Some(sig.clone());
            }
            // And under loaded packages, stripped name (a qualified call
            // to a package we have signatures for but where the function
            // lives under a different name is unlikely, but be thorough).
            for pk in self.available_package_names() {
                if !self.bare_loaded.contains(pk) {
                    continue;
                }
                if let Some(t) = self.package_typeshed(pk) {
                    if let Some(sig) = t.functions.get(fun) {
                        return Some(sig.clone());
                    }
                }
            }
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
        self.fn_table.fns.contains_key(name)
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
        if self.discarding {
            // Pass 2 (fixpoint) and closure-signature building run the
            // single inference engine in "discarding" mode: types are
            // computed but no diagnostics are recorded. This keeps pass 2
            // from double-emitting (diagnostics are produced in pass 3
            // against the refined FnTable).
            return;
        }
        self.diagnostics
            .push(Diagnostic::new(severity, span, &self.path, code, msg));
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
