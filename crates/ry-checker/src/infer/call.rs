use super::callee::{CallHead, CallHeadFunctionEvidence};
use super::*;
use crate::definitions::{S4Call, S4Selection};
use crate::higher_order::s3_group_generic;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr};
use std::ops::ControlFlow;

impl Checker {
    pub(crate) fn infer_call(
        &mut self,
        func: &Expr,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        // R fixes the call head before promises can mutate its binding,
        // so an error-assertion wrapper (#344) is recognized here too.
        // The wrapper owns the probe push/pop so early returns in the
        // body cannot leak a span onto the stack.
        if let Some(probe) = self.error_probe_span(func, args, scope) {
            self.assertion_probe_spans.push(probe);
            let result = self.infer_call_body(func, args, scope, span);
            self.assertion_probe_spans.pop();
            result
        } else {
            self.infer_call_body(func, args, scope, span)
        }
    }

    fn infer_call_body(
        &mut self,
        func: &Expr,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        if self.capture_references {
            self.enable_eager_reference_argument(func, args, scope, span);
        }
        // Calls may install delayed bindings, not just mutate current values.
        // Preserve environment certainty only for a closed literal constructor.
        let environment_known_before_call = !scope.uncertainty.ops_environment;
        let pure = ops_chooser::pure_literal_constructor(self, func, args, scope);
        if !pure {
            scope.invalidate_literal_values();
            scope.invalidate_ops_environment();
        }
        // R fixes the call head before promises can mutate its binding, so
        // classify the effect now and apply it after the call is checked.
        let effect = self.call_effect(func, args, scope);
        // Related-location evidence (#672) survives only an audited base
        // call, never an inert user helper or `message()`.
        let keeps_bindings = !(scope.uncertainty.dynamic_bindings
            && scope.uncertainty.unseen_rebinds)
            && self.call_proves_no_rebind(func, args, scope);
        let kind = self.base_value_kind(func, args, scope);
        let result = self.infer_call_inner(func, args, scope, span, environment_known_before_call);
        let result = kind.unwrap_or(result);
        scope.uncertainty.unseen_rebinds |= !keeps_bindings;
        self.apply_call_effect(&effect, scope);
        if !pure {
            scope.invalidate_ops_environment();
        }
        result
    }

    /// The span of the probe argument when this call asserts or catches
    /// an error by construction (#344): testthat's `expect_error`,
    /// `expect_warning`, and `expect_snapshot(error = TRUE)`, plus base
    /// `try` and `tryCatch` with an `error` handler. The formals listed
    /// are only those the probe and assertion bindings need; matching
    /// follows R, so named and partial forms bind the same probe as the
    /// positional one. `expect_snapshot` asserts an error only with a
    /// literal `error = TRUE`; any other value may be false at runtime.
    fn error_probe_span(
        &self,
        func: &Expr,
        args: &[Arg],
        scope: &Scope,
    ) -> Option<(Span, ProbeKind)> {
        let name = callee_name(func)?;
        let lookup_name = crate::semantic_lists::bare_name(&name);
        if !matches!(
            lookup_name,
            "expect_error" | "expect_warning" | "expect_snapshot" | "tryCatch" | "try"
        ) {
            return None;
        }
        let semantic_name: &str = scope.function_alias(&name).unwrap_or(&name);
        // An alias cannot prove the captured callable is the wrapper.
        if name != semantic_name || is_user_infix_name(semantic_name) {
            return None;
        }
        let (package, formals): (&str, &[&str]) = match lookup_name {
            // Real testthat 3.x formals (R/expect-condition.R), `class`
            // and `inherit` included so partial names bind as in R.
            "expect_error" => (
                "testthat",
                &[
                    "object", "regexp", "class", "...", "inherit", "info", "label",
                ],
            ),
            "expect_warning" => (
                "testthat",
                &[
                    "object", "regexp", "class", "...", "inherit", "all", "info", "label",
                ],
            ),
            // The real signature: `x` is the captured code; there is
            // no `...`, so partial names bind post-`x` formals as in R.
            "expect_snapshot" => (
                "testthat",
                &["x", "cran", "error", "transform", "variant", "cnd_class"],
            ),
            "tryCatch" => ("base", &["expr", "...", "finally"]),
            "try" => ("base", &["expr", "silent", "outFile"]),
            _ => return None,
        };
        if !self.error_wrapper_recognized(&name, func, semantic_name, lookup_name, package, scope) {
            return None;
        }
        let matched = match_arguments(formals, args);
        let arg_named = |param: &str| {
            formals
                .iter()
                .position(|formal| *formal == param)
                .and_then(|idx| matched.arg_for_param(idx))
        };
        let asserts = match lookup_name {
            // `regexp = NA` asserts that NO error/warning occurs, the
            // opposite probe; a proven-`NA` regexp refuses, anything
            // else (absent/`NULL`/string/dynamic) keeps the probe.
            "expect_error" | "expect_warning" => match arg_named("regexp") {
                None => true,
                Some(arg) => !matches!(args[arg].value, Expr::Na(..)),
            },
            // Only literal TRUE asserts: current testthat opens with
            // `check_bool(error)`, so a numeric `error` is rejected
            // before the probe is evaluated and asserts nothing.
            "expect_snapshot" => arg_named("error")
                .is_some_and(|arg| matches!(args[arg].value, Expr::Logical(true, _))),
            // `error` reaches tryCatch via `...`, so R requires the
            // exact name — partial matching only binds real formals. A
            // literal non-function handler (`NULL`, `NA`, `"x"`, `1`,
            // `TRUE`) catches nothing in R -- it errors as "attempt to
            // apply non-function" -- so it does not make `expr` a probe.
            // A `condition` handler catches errors too (the parent
            // class), so it asserts the same way.
            "tryCatch" => args.iter().any(|arg| {
                matches!(arg.name.as_deref(), Some("error" | "condition"))
                    && !matches!(
                        arg.value,
                        Expr::Missing(_)
                            | Expr::Null(_)
                            | Expr::Na(..)
                            | Expr::Logical(..)
                            | Expr::Integer(..)
                            | Expr::Double(..)
                            | Expr::String(..)
                    )
            }),
            _ => true,
        };
        if !asserts {
            return None;
        }
        let probe = matched.arg_for_param(0)?;
        let kind = if lookup_name == "expect_warning" {
            ProbeKind::Warning
        } else {
            ProbeKind::Error
        };
        Some((span_of(&args[probe].value), kind))
    }

    /// Whether the callee is the real testthat/base wrapper rather than
    /// a masked same-named function. Proven provenance decides most
    /// calls. Under ambient uncertainty the expected package must be
    /// attached (the state of real test files, where testthat is on the
    /// search path and its exports are external bindings) and no
    /// concrete mask may be visible: a project or lexical definition, a
    /// formal, a function-valued binding, another package's import, a
    /// user stub for the package, or an unknown data mask all refuse.
    /// A qualified call requires proven provenance, so a masked `::`
    /// never qualifies. This mirrors the testthat block verbs, which
    /// also apply their semantics under ambient uncertainty once a
    /// project-local definition is excluded.
    fn error_wrapper_recognized(
        &self,
        name: &str,
        func: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        package: &str,
        scope: &Scope,
    ) -> bool {
        use crate::resolve::SpecialCallProvenance;
        match self.special_call_provenance(name, func, semantic_name, lookup_name, package, scope) {
            SpecialCallProvenance::Proven => true,
            SpecialCallProvenance::Ordinary => false,
            SpecialCallProvenance::AmbientUncertainty | SpecialCallProvenance::Unknown => {
                // R's function-mode lookup skips a binding known to hold
                // data, so only a possibly-function binding can mask.
                // Positive match on the possibly-function modes, so a
                // future data mode does not silently count as callable.
                let binding_may_be_function = scope
                    .get(semantic_name)
                    .is_some_and(|ty| ty.fn_sig.is_some() || ty.mode.may_be_function());
                !semantic_name.contains("::")
                    && !scope.data_mask_unknown
                    && !scope.is_parameter(semantic_name)
                    && scope.lexical_definition(semantic_name).is_none()
                    && !self.fn_table.functions.binds(semantic_name)
                    && !binding_may_be_function
                    && !self.user_stubs.contains_key(package)
                    && !self.user_stubs.contains_key("base")
                    && self
                        .imported_from
                        .get(semantic_name)
                        .is_none_or(|imported| imported == package)
                    && (package == "base"
                        || self.bare_loaded.contains(package)
                        || self.loaded.contains(package))
            }
        }
    }

    /// Discard declaration-only exact identities after evaluating an
    /// expression whose binding effects cannot be excluded. This extra
    /// invalidation runs only when an adopted contract is selected.
    pub(crate) fn invalidate_declaration_identities(&self, scope: &mut Scope) {
        if self.discarding || !self.declarations.has_contracts() {
            return;
        }
        let mut names = scope
            .lexical_definitions
            .keys()
            .cloned()
            .collect::<FxSet<_>>();
        names.extend(self.adopted_function_names());
        for name in names {
            scope.insert(name, RType::unknown());
        }
    }

    /// Project function names whose definition has an adopted signature.
    pub(crate) fn adopted_function_names(&self) -> impl Iterator<Item = String> + '_ {
        self.fn_table
            .functions
            .live()
            .filter(|(_, function)| {
                self.declarations
                    .target(&function.source_path, function.definition_span)
                    .is_some_and(|decision| decision.signature.is_some())
            })
            .map(|(name, _)| semantic_argument_name(name).to_string())
    }

    pub(crate) fn infer_call_inner(
        &mut self,
        func: &Expr,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
        environment_known_before_call: bool,
    ) -> RType {
        // A call to a function literal (IIFE) never reaches the
        // name-based stages below.
        if let Expr::Function { .. } = func {
            return self.infer_function_literal_call(func, args, scope);
        }

        if let Some(result) =
            self.infer_box_call(func, args, scope, span, environment_known_before_call)
        {
            return result;
        }

        // Only model direct calls `name(...)`. Pipelines and indirect calls
        // return opaque.
        let Some(name) = callee_name(func) else {
            return self.infer_opaque_callee(func, args, scope, span);
        };

        // RY123: the head itself is a namespace access. A same-spelled
        // local or project function is selected instead (call-head rule),
        // so only an unshadowed qualified head reaches the namespace.
        self.maybe_check_failed_namespace_access(&name, span_of(func), scope);

        // An explicitly qualified typed package value is known not to be
        // callable. Bare names wait until after lexical callable lookup below,
        // because a local function or function-valued formal may legitimately
        // shadow an attached package constant.
        if name.contains("::")
            && let Some(value_type) = self.resolve_typeshed_value(&name)
        {
            let result = self.emit_not_callable(&name, value_type.mode, span);
            self.infer_args_for_diagnostics(args, scope);
            return result;
        }

        // Callee selection retains the full semantic name and its package.
        // The constructor and dispatch stages also need the bare spelling.
        let semantic_name = scope.function_alias(&name).unwrap_or(&name).to_string();
        let lookup_name = if is_user_infix_name(&semantic_name) {
            // `%::%` is an infix operator, not a namespace-qualified call.
            // Do not split the `::` embedded in its operator spelling.
            semantic_name.clone()
        } else {
            crate::semantic_lists::bare_name(&semantic_name).to_string()
        };

        if lookup_name == "local"
            && let Some(result) =
                self.infer_local_environment_call(&name, func, &semantic_name, args, scope)
        {
            return result;
        }

        // testthat's block verbs evaluate their body in a fresh child
        // environment (#368). Must run before the argument-inference stage
        // below, whose eager walk would flatten the block into the caller.
        if let Some(result) =
            self.infer_testthat_block_call(&name, func, &semantic_name, &lookup_name, args, scope)
        {
            return result;
        }

        // `foreach(iter = xs, ...) %op% { ... }` evaluates the RHS with each
        // named iteration argument bound. Must run before the unknown-infix
        // quoting stage below, or an unrecognized `%do%`/`%dopar%` would
        // quote the loop body and discard its diagnostics.
        if let Some(t) = self.infer_foreach_infix_call(&semantic_name, args, scope) {
            return t;
        }

        if let Some(t) = self.infer_quoted_infix_call(&semantic_name, args, scope) {
            return t;
        }

        if let Some(result) =
            self.infer_injected_call(&semantic_name, &lookup_name, args, scope, span)
        {
            return result;
        }

        // `assign("f", ..., envir = asNamespace("pkg"))` registers an
        // exported name. Must run before the default two-argument `assign`
        // stage below: a call carrying an `envir` control would otherwise be
        // treated as a local rebind.
        if let Some(t) = self.infer_namespace_registration_assign(&lookup_name, args, scope) {
            return t;
        }

        let base_aggregate = lookup_name == "length"
            && self.boolean_context_span == Some(span)
            && matches!(
                self.special_call_provenance(
                    &name,
                    func,
                    &semantic_name,
                    &lookup_name,
                    "base",
                    scope,
                ),
                crate::resolve::SpecialCallProvenance::Proven
            );
        self.check_comparison_inside_aggregate(&lookup_name, args, scope, span, base_aggregate);
        self.check_comparison_inside_math_fn(&lookup_name, args);
        self.check_identical_list_subset(&lookup_name, args, scope);
        self.check_testthat_self_comparison(&name, func, &semantic_name, &lookup_name, args, scope);
        self.check_dimensional_on_function(&name, func, &semantic_name, &lookup_name, args, scope);
        self.check_cli_template(&name, func, &semantic_name, &lookup_name, args, scope);

        if matches!(lookup_name.as_str(), "hasArg" | "on.exit") {
            let package = if lookup_name == "hasArg" {
                "methods"
            } else {
                "base"
            };
            let proven = match self.special_call_provenance(
                &name,
                func,
                &semantic_name,
                &lookup_name,
                package,
                scope,
            ) {
                crate::resolve::SpecialCallProvenance::Proven => Some(true),
                // Preserve the existing deferred model under ambient uncertainty.
                // Namespace imports and whole-file attachment metadata do not yet
                // establish call-time lookup. Do not claim RY096 proof here.
                crate::resolve::SpecialCallProvenance::AmbientUncertainty
                    if !self.user_stubs.contains_key(package)
                        && !self.user_stubs.contains_key("base") =>
                {
                    Some(false)
                }
                crate::resolve::SpecialCallProvenance::Ordinary
                | crate::resolve::SpecialCallProvenance::AmbientUncertainty => None,
                crate::resolve::SpecialCallProvenance::Unknown => {
                    scope.invalidate_unknown_effects();
                    return RType::unknown();
                }
            };
            if let Some(proven) = proven
                && let Some(t) = self.infer_deferred_call(&lookup_name, args, scope, span, proven)
            {
                return t;
            }
        }

        if matches!(lookup_name.as_str(), "sprintf" | "gettextf")
            && matches!(
                self.special_call_provenance(
                    &name,
                    func,
                    &semantic_name,
                    &lookup_name,
                    "base",
                    scope
                ),
                crate::resolve::SpecialCallProvenance::Proven
            )
        {
            self.check_printf_format_arity(&lookup_name, args);
        }

        if let Some(t) =
            self.infer_nse_quoting_call(&semantic_name, &lookup_name, args, scope, span)
        {
            return t;
        }

        // Proved base switch selects before evaluating its alternatives.
        if lookup_name == "switch"
            && let Some(result) = self.infer_switch_special(
                &name,
                func,
                &semantic_name,
                args,
                scope,
                environment_known_before_call,
            )
        {
            return result;
        }

        // `tryCatch(expr, ..., handler = fun)`: the join of the main
        // expression and all handler return types.
        if semantic_name == "tryCatch" {
            return self.infer_trycatch_call(args, scope);
        }

        // The class-constructor stage: `structure`, `factor`, S4 `new`.
        if let Some(t) = self.infer_class_constructor_call(
            &name,
            func,
            &semantic_name,
            &lookup_name,
            args,
            scope,
        ) {
            return t;
        }

        // `assign`/`delayedAssign` into the current frame bind the target name.
        if let Some(t) = self
            .infer_local_assign_call(&semantic_name, args, scope)
            .or_else(|| self.infer_local_delayed_assign_call(&semantic_name, args, scope))
        {
            return t;
        }

        // NSE verbs (`subset`, `with`, `within`, `transform`) evaluate
        // their expression arguments in a data-mask scope. Must run
        // before the argument-inference stage below, whose eager infer
        // loop would emit spurious RY010 for every column reference.
        if let Some(t) = self.infer_schema_call(&semantic_name, args, scope) {
            return t;
        }

        // The argument-inference stage.
        let mut call =
            self.infer_argument_types(&name, &semantic_name, &lookup_name, args, scope, span);

        // The argument-validation stage.
        self.check_call_arguments(&name, &lookup_name, &call.head, &call.arg_types, args, span);

        // The dynamic-loader stage; also records `locally_shadows_stub`
        // on the resolution for the assertion stage below.
        self.note_dynamic_loader_scope(&name, &mut call, args, scope);

        // Assertion narrowing must run before the dispatch tail below:
        // `assert_that`/`stopifnot` calls also resolve as ordinary typeshed
        // or FnTable functions, and the tail would return their result type
        // without narrowing the subject binding.
        if let Some(t) = self.infer_assert_scalar_call(&lookup_name, args, scope) {
            return t;
        }
        if let Some(t) = self.infer_stub_assertion_call(&name, &lookup_name, &call, args, scope) {
            return t;
        }
        self.apply_assertion_predicates(&name, args, scope);

        // A project `UseMethod` generic returns whatever its selected method
        // returns, so dispatch before the lexical stage reads the generic's
        // own opaque return.
        if let Some(receiver) = call.head.generic_receiver {
            if self.caller_method_may_shadow(&lookup_name, &call.arg_types[receiver], scope) {
                return RType::unknown();
            }
            let mut dispatch_args = call.arg_types.clone();
            dispatch_args[..=receiver].rotate_right(1);
            if let Some(rt) = self.try_s3_dispatch(&lookup_name, &dispatch_args, span) {
                return rt;
            }
        }

        // A contract describes the selected definition, which is also the
        // lexical binding whenever one exists.
        if let Some(result) = self.contract_return(&call.head, args, &call.arg_types) {
            return result;
        }

        // The lexical-callable stage. Must run before the constructor and
        // dispatch stages below so a local binding shadows a same-named
        // builtin (R's lexical scoping).
        if let Some(t) =
            self.infer_lexical_callable_call(&name, args, &call.arg_types, span, &call.head)
        {
            return t;
        }

        // No lexical callable won. A bare typed package value is a
        // non-function call, not a zero-argument function signature --
        // but only when no function of that name is reachable anywhere
        // (#384): package-own definitions live in the FnTable, and R's
        // function-mode call lookup skips the dataset binding in favor
        // of any such outward function. An uncertain outward state
        // (open search path, unenumerable data mask) silences too.
        if call.head.lexical_binding.is_none()
            && let Some(value_type) = self.resolve_typeshed_value(&name)
            && matches!(
                call.head.function_evidence,
                CallHeadFunctionEvidence::Diagnose
            )
        {
            return self.emit_symbol_not_callable(&name, value_type.mode, span);
        }

        // The atomic-constructor stage: `c`, `list`, `data.frame`, `t`,
        // `as.data.frame`.
        if let Some(t) = self.infer_atomic_constructor_call(&lookup_name, args, &call.arg_types) {
            return t;
        }

        // Class dispatch must run before the higher-order, FnTable, and
        // typeshed stages below: a method's inferred return type wins over
        // the generic's stub.
        let instances: Vec<bool> = args
            .iter()
            .map(|argument| self.is_proven_new_call(&argument.value, scope))
            .collect();
        let s4_call = S4Call {
            head: Callee::parse(&semantic_name),
            own_package: self.package_facts.source_package.as_deref(),
            args,
            arg_types: &call.arg_types,
            instances: &instances,
            core_formals: self
                .typeshed
                .functions
                .get(&lookup_name)
                .map(|signature| signature.params.as_slice()),
        };
        match self.fn_table.functions.select_s4(s4_call) {
            S4Selection::Method(method) => return self.read_return_slot(method.return_slot),
            S4Selection::Uncertain => return RType::unknown(),
            S4Selection::NoCandidate => {}
        }

        if let Some(rt) = call
            .arg_types
            .first()
            .and_then(|first| self.user_s3_dispatch_return(&lookup_name, first))
        {
            return rt;
        }

        // S3 dispatch: when a known generic is called with a classed
        // first argument, look up `(generic, class)` in the S3 method
        // table (walking the class vector, then the Math/Summary group
        // fallback). On a hit, return the method's inferred return type.
        // On a miss with a *known* class, emit RY050. On a miss with an
        // unknown or empty class, fall through (we can't say anything).
        // The prefix-stripped `lookup_name` is used so `base::print(x)`
        // dispatches as `print`.
        if self
            .typeshed
            .globals
            .s3_generics
            .iter()
            .any(|generic| generic == &lookup_name)
            || s3_group_generic(&lookup_name).is_some()
        {
            if let Some(rt) = self.try_s3_dispatch(&lookup_name, &call.arg_types, span) {
                return rt;
            }
            if call
                .arg_types
                .first()
                .is_some_and(|argument| argument.class.is_unknown())
            {
                return RType::unknown();
            }
        }

        // Higher-order built-ins (`lapply`, `sapply`, `vapply`, `Map`,
        // `Reduce`, `Filter`, ...): model the callback to infer the
        // result type; the callback body is walked first so RY010 on an
        // unbound name inside it still fires. Must run before the
        // FnTable stage below so a project-local `lapply` wrapper still
        // gets callback inference.
        if let Some(signature) = call.head.signature()
            && let Some(result) = self.infer_higher_order_call(
                &semantic_name,
                signature,
                args,
                &call.arg_types,
                scope,
                span,
            )
        {
            return result;
        }

        // User functions: the refined FnTable return slot (stabilized by
        // the fixpoint loop in `check()`; refining on demand would risk
        // exponential blowup).
        if let Some(function) = call.head.user_function() {
            let result = self.read_return_slot(function.return_slot);
            // A callee without a computed return yet has no known way to
            // return, so this path contributes nothing to merges. A
            // qualified call may name another package's same-named function.
            if !name.contains("::") && self.uncomputed_returns.contains(&function.return_slot) {
                scope.unreachable = true;
            }
            return result;
        }

        // The literal-length constructor stage: `vector`, `rep`, `seq`.
        // Must run after the FnTable stage above so a user-defined
        // `rep`/`seq` still wins, and before the typeshed stage below so
        // the literal-pinned length beats the conservative stub.
        if let Some(t) = self.infer_literal_length_call(&lookup_name, args, &call.arg_types) {
            return t;
        }

        // The test-template mode stage: a signature declaring a
        // `return_mode` `test_template` rule (`base::ifelse`) seeds its
        // result from the test vector, so the declared branch-mode join
        // collapses to logical when the test may be empty or is all-NA
        // (RY106). Runs after every dispatch shape above so only a call
        // that reaches the plain typeshed stage is claimed.
        if let Some(signature) = call.head.signature()
            && signature.return_mode.is_some()
            && let Some(result) = self.infer_test_template_call(
                &lookup_name,
                signature,
                args,
                &call.arg_types,
                scope,
                span,
            )
        {
            return result;
        }

        // A base integer cast can create an NA even from a finite double.
        // Keep that fact separate from the integer return mode, and decline
        // to model classed inputs whose S3 method may change the result.
        if lookup_name == "as.integer"
            && matches!(
                self.special_call_provenance(
                    &name,
                    func,
                    &semantic_name,
                    &lookup_name,
                    "base",
                    scope,
                ),
                crate::resolve::SpecialCallProvenance::Proven
            )
            && let Some(input) = call.arg_types.first()
        {
            use ry_core::types::NewNaProvenance;
            let mut result = RType::new(Mode::Integer, input.length);
            result.value_facts.prior_na = input.value_facts.prior_na;
            result.value_facts.all_values_known = input.value_facts.all_values_known;
            let plain_numeric = matches!(input.mode, Mode::Double | Mode::Integer)
                && !input.class.is_unknown()
                && !input.class.has_known_class();
            let mut introduced_here = false;
            result.value_facts.new_na =
                match input.value_facts.numeric_bounds.filter(|_| plain_numeric) {
                    Some((low, high)) => {
                        let low = f64::from_bits(low);
                        let high = f64::from_bits(high);
                        if low <= -2_147_483_648.0 || high >= 2_147_483_648.0 {
                            introduced_here = true;
                            if (low >= 2_147_483_648.0 || high <= -2_147_483_648.0)
                                && !input.value_facts.prior_na
                            {
                                NewNaProvenance::ProvenOnly
                            } else {
                                NewNaProvenance::ProvenContains
                            }
                        } else {
                            result.value_facts.numeric_bounds =
                                Some((low.trunc().to_bits(), high.trunc().to_bits()));
                            input.value_facts.new_na
                        }
                    }
                    None if matches!(
                        input.value_facts.new_na,
                        NewNaProvenance::ProvenContains | NewNaProvenance::ProvenOnly
                    ) =>
                    {
                        input.value_facts.new_na
                    }
                    None if plain_numeric && input.value_facts.all_values_known => {
                        NewNaProvenance::None
                    }
                    None if plain_numeric && input.mode == Mode::Integer => {
                        input.value_facts.new_na
                    }
                    None if input.mode == Mode::Null
                        || matches!(input.length, Length::Zero | Length::Known(0)) =>
                    {
                        NewNaProvenance::None
                    }
                    None => NewNaProvenance::Possible,
                };
            if introduced_here {
                match &mut self.held_cast {
                    Some(held) if held.span == span => held.warned = true,
                    _ => self.emit_integer_range_loss(span),
                }
            }
            return result;
        }

        // The typeshed stage: a qualified call (`pkg::fun`) resolves
        // against `load_package(pkg)`; an unqualified call falls back
        // from base to loaded packages (reverse load order).
        if let Some(sig) = call.head.signature() {
            return self.apply_sig(sig, &call.arg_types, args);
        }

        // Unknown function: opaque.
        RType::unknown()
    }

    /// A call to a function literal (an IIFE): infer via
    /// `callback_return_type`, which walks the body with the params bound
    /// to the actual argument types.
    fn infer_function_literal_call(
        &mut self,
        func: &Expr,
        args: &[Arg],
        scope: &mut Scope,
    ) -> RType {
        let arg_types: Vec<RType> = args.iter().map(|a| self.infer(&a.value, scope)).collect();
        if let Some(rt) = self.callback_return_type(func, &arg_types, scope) {
            return rt;
        }
        RType::unknown()
    }

    /// A callee that is not a name: a literal value errors at runtime
    /// (RY070); indirect callees stay silent and opaque.
    fn infer_opaque_callee(
        &mut self,
        func: &Expr,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        if let Some(mode) = literal_callee_mode(func) {
            self.emit(
                Severity::Error,
                span,
                "RY070",
                format!("cannot call a value of mode `{}`", mode),
            );
            self.infer_args_for_diagnostics(args, scope);
            return RType::unknown();
        }
        self.infer(func, scope);
        for argument in args {
            self.infer_with_injection(&argument.value, scope, Some(InjectionMode::Full));
        }
        RType::unknown()
    }

    /// Base `local` evaluates `expr` in a fresh environment by default.
    fn infer_local_environment_call(
        &mut self,
        name: &str,
        func: &Expr,
        semantic_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        use crate::resolve::SpecialCallProvenance;
        match self.special_call_provenance(name, func, semantic_name, "local", "base", scope) {
            SpecialCallProvenance::Ordinary => return None,
            SpecialCallProvenance::Unknown | SpecialCallProvenance::AmbientUncertainty => {
                // The callee may force arguments in the caller or evaluate
                // them in a child. Only names written by the arguments lose
                // their caller-side value facts.
                let mut child = scope.function_execution_scope();
                let mark = child.begin_snapshot();
                for argument in args {
                    self.infer(&argument.value, &mut child);
                }
                let delta = child.finish_snapshot(mark, Default::default());
                scope.uncertainty.effects |= delta.uncertainty.effects;
                if delta.data_mask_env_unknown {
                    scope.invalidate_data_mask_env();
                }
                for name in delta.changed.into_keys() {
                    scope.insert(name.clone(), RType::unknown());
                    scope.possible_block_writes.insert(name);
                }
                return Some(RType::unknown());
            }
            SpecialCallProvenance::Proven => {}
        }
        let bindings = match_arguments(&["expr", "envir"], args);
        if bindings.param_for_arg.iter().any(Option::is_none) {
            return None;
        }
        let expression = bindings.arg_for_param(0)?;
        let block = &args[expression].value;
        let mut child = scope.function_execution_scope();
        if let Some(environment) = bindings.arg_for_param(1) {
            let value = &args[environment].value;
            let caller_environment = matches!(value, Expr::Call { func, args, .. }
                if args.is_empty() && callee_name(func).is_some_and(|name|
                    crate::semantic_lists::bare_name(&name) == "environment"
                    && self.resolves_to_base(&name, scope)));
            self.infer(value, scope);
            if caller_environment {
                return Some(self.infer(block, scope));
            }
            // An explicit environment can have unrelated bindings and parents.
            child.invalidate_unknown_effects();
        }
        let effects_were_unknown = child.uncertainty.effects;
        let result = self.infer(block, &mut child);
        if child.uncertainty.effects && !effects_were_unknown {
            // Eager nonlocal writes can reach the caller from this child.
            scope.invalidate_unknown_effects();
        }
        // Nested closures defined inside the block may run after `local`
        // returns and climb out through the block's frame (issue #374),
        // so their `<<-` targets reach the caller too -- except the
        // writes an intervening frame's formal intercepts, which the
        // collector prunes.
        index::superassignment_writes_in_expr(block).apply(scope);
        Some(result)
    }

    /// testthat's block verbs — `test_that(desc, code)` plus the
    /// `describe`/`it` blocks built on the same `test_code` engine —
    /// evaluate the body through `eval(code, new.env(parent =
    /// caller_env))`. Bindings created inside one block never reach a
    /// sibling block or the file top level (#368), so a loop variable in
    /// one `test_that()` must not shadow a package symbol called in
    /// another. The body therefore walks a function-execution child
    /// scope: only `<<-` writes and unknown effects flow back.
    ///
    /// The evaluation contract belongs to these callees, not to braced
    /// arguments in general (#350): `identity({x <- 1})` writes through
    /// to the caller and keeps the ordinary argument path below.
    fn infer_testthat_block_call(
        &mut self,
        name: &str,
        func: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        if !crate::semantic_lists::is_testthat_block_verb(lookup_name) {
            return None;
        }
        use crate::resolve::SpecialCallProvenance;
        match self.special_call_provenance(
            name,
            func,
            semantic_name,
            lookup_name,
            "testthat",
            scope,
        ) {
            SpecialCallProvenance::Ordinary => return None,
            SpecialCallProvenance::Unknown | SpecialCallProvenance::AmbientUncertainty => {
                // The callee may be another package's same-named verb
                // whose block runs in the caller. Only names written by
                // the arguments lose their caller-side value facts.
                let mut child = scope.function_execution_scope();
                let mark = child.begin_snapshot();
                for argument in args {
                    self.infer(&argument.value, &mut child);
                }
                let delta = child.finish_snapshot(mark, Default::default());
                scope.uncertainty.effects |= delta.uncertainty.effects;
                if delta.data_mask_env_unknown {
                    scope.invalidate_data_mask_env();
                }
                for changed in delta.changed.into_keys() {
                    scope.insert(changed.clone(), RType::unknown());
                    scope.possible_block_writes.insert(changed);
                }
                return Some(RType::unknown());
            }
            SpecialCallProvenance::Proven => {}
        }
        let formals: Vec<String> = self
            .package_typeshed("testthat")?
            .functions
            .get(lookup_name)?
            .param_names()
            .map(str::to_owned)
            .collect();
        let formals: Vec<&str> = formals.iter().map(String::as_str).collect();
        let bindings = match_arguments(&formals, args);
        if bindings.param_for_arg.iter().any(Option::is_none) {
            return None;
        }
        let code_formal = formals.iter().position(|formal| *formal == "code");
        // The description promise is forced in the caller's frame.
        for (argument, formal) in args.iter().zip(&bindings.param_for_arg) {
            if *formal != code_formal {
                self.infer(&argument.value, scope);
            }
        }
        // The body's value is discarded; test_that returns its own
        // (invisible) logical outcome, which no caller pattern relies on.
        let Some(code) = code_formal.and_then(|formal| bindings.arg_for_param(formal)) else {
            return Some(RType::unknown());
        };
        let mut child = scope.function_execution_scope();
        let effects_were_unknown = child.uncertainty.effects;
        self.infer(&args[code].value, &mut child);
        if child.uncertainty.effects && !effects_were_unknown {
            // Eager nonlocal writes can reach the caller from this child.
            scope.invalidate_unknown_effects();
        }
        // Nested closures defined inside the block may run after the
        // block completes and climb out through its frame (issue #374),
        // so their `<<-` targets reach the caller too -- except the
        // writes an intervening frame's formal intercepts, which the
        // collector prunes.
        index::superassignment_writes_in_expr(&args[code].value).apply(scope);
        Some(RType::unknown())
    }

    /// `foreach(iter = xs, ...) %op% { ... }`: infer the RHS with each
    /// named iteration argument bound. The foreach-shaped LHS is
    /// recognized rather than a fixed `%do%`/`%dopar%` spelling; `%:%`
    /// chains contribute bindings from every constituent foreach call.
    fn infer_foreach_infix_call(
        &mut self,
        semantic_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        if !is_user_infix_name(semantic_name) || args.len() != 2 {
            return None;
        }
        let bindings = foreach_iteration_bindings(&args[0].value)?;
        let _ = self.infer(&args[0].value, scope);
        // Sequential `%do%` writes into the caller's frame. Other backends
        // may use a child environment with access to the caller's functions.
        let mut local = if semantic_name == "%do%" {
            scope.independent_execution_scope()
        } else {
            scope.function_execution_scope()
        };
        for binding in bindings {
            local.insert(binding, RType::unknown());
        }
        Some(self.infer(&args[1].value, &mut local))
    }

    /// Unknown custom infix operators are commonly small DSLs that quote
    /// both operands with `match.call()` or `substitute()`. Treating their
    /// operands as ordinary R expressions produces false positives for
    /// DSL-only names and operations (for example lambda.r declarations
    /// and plyr's formula-like helpers). They are language objects, so
    /// infer them only to preserve traversal invariants and never emit a
    /// diagnostic from inside either operand.
    ///
    /// A user-defined operator or a typeshed-known one remains an ordinary
    /// evaluated call: `has_function_anywhere` covers both sources. `.()`
    /// is the analogous quoting helper used by plyr/data.table.
    fn infer_quoted_infix_call(
        &mut self,
        semantic_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        if !is_user_infix_name(semantic_name) && semantic_name != "." {
            return None;
        }
        let custom_infix_is_known = self.has_function_anywhere(semantic_name)
            || self
                .fn_table
                .functions
                .names()
                .any(|name| semantic_argument_name(name) == semantic_name);
        if !custom_infix_is_known {
            let mut quoted_scope = scope.independent_execution_scope();
            for argument in args {
                self.infer_discarding(&argument.value, &mut quoted_scope);
            }
            return Some(RType::unknown());
        }
        None
    }

    /// Infer a call whose signature declares injected names (`injects`):
    /// stub-driven NSE such as `R6Class()` and formula interfaces. Returns
    /// `None` when no reachable signature declares an injection.
    fn infer_injected_call(
        &mut self,
        name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> Option<RType> {
        let signature = self.resolve_typeshed_sig(name).or_else(|| {
            self.available_package_names()
                .into_iter()
                .find_map(|package| {
                    // Cheap prefilter: an embedded stub that declares no
                    // `injects` cannot match, and skipping it avoids
                    // parsing the package on this scan.
                    if !self.package_may_inject(package) {
                        return None;
                    }
                    self.package_typeshed(package)
                        .and_then(|typeshed| typeshed.functions.get(lookup_name))
                        .filter(|signature| !signature.injects.is_empty())
                        .cloned()
                })
        })?;
        if signature.injects.is_empty() {
            return None;
        }
        let params: Vec<&str> = signature.param_names().collect();
        let matches = match_params(&signature.params, args);
        // R6 has two evaluation models for method bodies. Under the default
        // (`portable = TRUE`) a method is enclosed by a separate environment
        // in which members are reachable only through `self$` / `private$`,
        // so a bare member name is genuinely unbound. With
        // `portable = FALSE` the object's own environment is the enclosure,
        // so every public, private and active member -- fields and sibling
        // methods alike -- is in scope as a bare name.
        let member_bindings =
            if lookup_name == "R6Class" && r6_call_is_non_portable(args, &params, &matches) {
                self.r6_member_bindings(args, &params, &matches, scope)
            } else {
                Vec::new()
            };
        let mut arg_types = Vec::with_capacity(args.len());
        for (index, argument) in args.iter().enumerate() {
            let parameter = matches.param_for_arg[index].and_then(|index| params.get(index));
            let quoted_expression = matches!(
                eval_mode_for_arg(&signature, &matches, index),
                Some(EvalMode::QuotedExpression)
            );
            let specs: Vec<_> = signature
                .injects
                .iter()
                .filter(|spec| {
                    parameter
                        .is_some_and(|parameter| spec.into.iter().any(|into| into == parameter))
                })
                .collect();
            if specs.is_empty() {
                arg_types.push(self.infer(&argument.value, scope));
                continue;
            }
            let mut child = scope.function_execution_scope();
            let injects_fixed_names = specs.iter().any(|spec| !spec.names.is_empty());
            for spec in specs {
                for source in &spec.strings_from {
                    for (source_index, source_argument) in args.iter().enumerate() {
                        let source_parameter =
                            matches.param_for_arg[source_index].and_then(|index| params.get(index));
                        if source_parameter.is_some_and(|parameter| *parameter == source) {
                            for binding in injected_string_bindings(&source_argument.value) {
                                child.insert(binding, RType::unknown());
                            }
                        }
                    }
                }
                for binding in &spec.names {
                    child.insert(binding.clone(), RType::unknown());
                }
            }
            // Non-empty for a non-portable `R6Class()` call only, and the
            // injected params there are exactly the three member lists.
            for (member, member_type) in &member_bindings {
                child.insert(member.clone(), member_type.clone());
            }
            arg_types.push(
                if injects_fixed_names
                    && quoted_expression
                    && matches!(argument.value, Expr::Ident { .. })
                {
                    // An injected expression can still be a bare captured
                    // symbol. It is not evaluated in the caller, so avoid
                    // reporting it unbound while retaining injected checking for
                    // blocks and function literals below.
                    RType::unknown()
                } else if injects_fixed_names {
                    self.infer_injected_expr(&argument.value, &mut child)
                } else if quoted_expression {
                    RType::unknown()
                } else {
                    self.infer(&argument.value, &mut child)
                },
            );
        }
        self.check_typeshed_call_arguments(lookup_name, &signature, args, &arg_types, span);
        Some(self.apply_sig(&signature, &arg_types, args))
    }

    /// `assign("name", ..., envir = asNamespace("pkg"))`: the binding
    /// becomes visible to qualified lookups only.
    fn infer_namespace_registration_assign(
        &mut self,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        if lookup_name != "assign" {
            return None;
        }
        let registers_namespace = args.iter().any(|arg| {
            arg.name.as_deref() == Some("envir")
                && matches!(
                    &arg.value,
                    Expr::Call { func, .. }
                        if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "asNamespace")
                )
        });
        if !registers_namespace {
            return None;
        }
        let binding = args.first().and_then(|arg| match &arg.value {
            Expr::String(name, _) => Some(name.clone()),
            _ => None,
        })?;
        for argument in args.iter().skip(1) {
            self.infer(&argument.value, scope);
        }
        scope.insert(binding, RType::unknown());
        Some(RType::unknown())
    }

    /// RY093: a comparison directly inside `length()` / `nchar()` reads as
    /// an element guard but counts coercion results. `sum(x > 0)` is the
    /// idiomatic R way to count matches, so `sum` is deliberately excluded
    /// from this mis-parenthesization family.
    fn check_comparison_inside_aggregate(
        &mut self,
        lookup_name: &str,
        args: &[Arg],
        scope: &Scope,
        call_span: Span,
        base_aggregate: bool,
    ) {
        if !matches!(lookup_name, "length" | "nchar") {
            return;
        }
        let Some(Expr::BinOp {
            op,
            lhs,
            rhs,
            span: comparison_span,
        }) = args.first().map(|arg| &arg.value)
        else {
            return;
        };
        if !is_comparison(*op) {
            return;
        }
        let proven_boolean_grep = lookup_name == "length"
            && self.boolean_context_span == Some(call_span)
            && base_aggregate
            && self.resolves_to_base(op_symbol(*op), scope)
            && !ops_chooser::operator_rebound(self, op_symbol(*op), scope)
            && self.proven_grep_position_call(*op, lhs, rhs, scope);
        if !proven_boolean_grep {
            let message = format!(
                "comparison is inside `{lookup_name}()`; compare `{lookup_name}(x)` instead"
            );
            self.emit(Severity::Warning, *comparison_span, "RY093", message);
        }
    }

    /// The result of ordinary `grep()` is a positive integer position for
    /// each match. Therefore `length(grep(...) > 0)` has the same boolean
    /// truth behavior as `length(grep(...)) > 0`, but only when `grep()` is
    /// proven to return positions. `value = TRUE` changes that contract to
    /// character results and remains under RY093.
    fn proven_grep_position_call(
        &self,
        op: BinOpKind,
        lhs: &Expr,
        rhs: &Expr,
        scope: &Scope,
    ) -> bool {
        let is_zero = |expr: &Expr| match expr {
            Expr::Integer(0, _) => true,
            Expr::Double(value, _) => *value == 0.0,
            _ => false,
        };
        let grep_expr = match op {
            BinOpKind::Gt | BinOpKind::Ge if is_zero(rhs) => lhs,
            BinOpKind::Lt | BinOpKind::Le if is_zero(lhs) => rhs,
            BinOpKind::Ne if is_zero(rhs) => lhs,
            BinOpKind::Ne if is_zero(lhs) => rhs,
            _ => return false,
        };
        let Expr::Call {
            func: grep_func,
            args: grep_args,
            ..
        } = grep_expr
        else {
            return false;
        };
        let Some(grep_name) = ident_name(grep_func) else {
            return false;
        };
        if crate::semantic_lists::bare_name(grep_name) != "grep"
            || !matches!(
                self.special_call_provenance(
                    grep_name, grep_func, grep_name, "grep", "base", scope,
                ),
                crate::resolve::SpecialCallProvenance::Proven
            )
        {
            return false;
        }

        // `grep`'s fifth formal is `value`; match it exactly as R does so
        // reordered and partial named arguments cannot bypass the control.
        let bindings = match_arguments(
            &[
                "pattern",
                "x",
                "ignore.case",
                "perl",
                "value",
                "fixed",
                "useBytes",
                "invert",
            ],
            grep_args,
        );
        // A failed match, duplicate formal, or missing required input means
        // R does not produce a positional result at all. Keep the exemption
        // out of those calls instead of proving a shape for an invalid call.
        if !bindings.is_one_to_one()
            || (0..=1).any(|formal| {
                bindings
                    .arg_for_param(formal)
                    .is_none_or(|index| matches!(grep_args[index].value, Expr::Missing(_)))
            })
        {
            return false;
        }
        // The position contract is independent of pattern/text values, but
        // unknown controls can still change whether the call succeeds. A
        // literal logical control keeps this proof deliberately bounded.
        if grep_args.iter().enumerate().any(|(index, argument)| {
            bindings.param_for_arg[index]
                .is_some_and(|formal| formal >= 2 && !matches!(argument.value, Expr::Logical(_, _)))
        }) {
            return false;
        }
        let Some(value_index) = bindings.arg_for_param(4) else {
            // The default is `value = FALSE`, so an omitted control still
            // produces integer positions.
            return true;
        };
        matches!(grep_args[value_index].value, Expr::Logical(false, _))
    }

    /// RY100: numeric math functions coerce logical comparisons to 0/1,
    /// which is almost always a misplaced parenthesis (`abs(x > y)` rather
    /// than `abs(x) > y`). Extra parentheses do not change the parsed
    /// argument, so deliberately parenthesized comparisons remain visible
    /// here.
    fn check_comparison_inside_math_fn(&mut self, lookup_name: &str, args: &[Arg]) {
        if crate::semantic_lists::BASE_MATH_FNS.contains(&lookup_name)
            && let Some(Expr::BinOp {
                op,
                span: comparison_span,
                ..
            }) = args.first().map(|arg| &arg.value)
            && is_comparison(*op)
        {
            let message = "comparison directly inside a numeric math function is usually a parenthesization mistake; compare the math result instead";
            self.emit(Severity::Warning, *comparison_span, "RY100", message);
        }
    }

    /// RY101: `x["name"]` preserves a list container, whereas `x[["name"]]`
    /// extracts its element. Comparing the former to an atomic scalar with
    /// `identical()` is therefore provably FALSE and commonly indicates a
    /// missing bracket.
    fn check_identical_list_subset(&mut self, lookup_name: &str, args: &[Arg], scope: &mut Scope) {
        if lookup_name == "identical"
            && args.len() >= 2
            && let Some(indexed) = args.iter().find(|argument| {
                matches!(
                    &argument.value,
                    Expr::Index {
                        kind: IndexKind::Single,
                        args,
                        ..
                    } if args.len() == 1
                        && matches!(args[0].value, Expr::String(_, _) | Expr::Integer(_, _) | Expr::Double(_, _))
                )
            })
            && args.iter().any(|argument| {
                !std::ptr::eq(argument, indexed)
                    && matches!(
                        argument.value,
                        Expr::Logical(_, _)
                            | Expr::Integer(_, _)
                            | Expr::Double(_, _)
                            | Expr::String(_, _)
                    )
            })
            && let Expr::Index { base, .. } = &indexed.value
        {
            let base_type = self.infer(base, scope);
            let list_origin = matches!(base_type.mode, Mode::List)
                || matches!(base.as_ref(), Expr::Ident { name, .. } if scope.has_list_origin(name));
            if list_origin {
                let message = "single-bracket list subset remains a list, so `identical()` with an atomic scalar is always FALSE; use `[[` to extract the element";
                self.emit(Severity::Warning, indexed.span, "RY101", message);
            }
        }
    }

    /// RY122: a resolved testthat equality assertion whose two compared
    /// operands are the same conservatively deterministic expression
    /// cannot check two different values (ggplot2 compared
    /// `round(breaks$major, 3)` to itself where `breaks$sec.major` was
    /// meant). The safe set is deliberately narrow: literals, plain
    /// variable reads, one `$`/`[`/`[[` step from a receiver proven to
    /// be plain data, and calls to the base numeric math functions
    /// RY100 already enumerates. Anything else -- RNG/time/I/O and
    /// unknown calls, operators that may dispatch, receivers whose
    /// class could carry a `$`/`[[` method, masked assertion
    /// functions -- stays silent; false negatives beat false positives.
    fn check_testthat_self_comparison(
        &mut self,
        name: &str,
        func: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &Scope,
    ) {
        if !matches!(lookup_name, "expect_equal" | "expect_identical")
            || scope.data_mask_unknown
            || !matches!(
                self.special_call_provenance(
                    name,
                    func,
                    semantic_name,
                    lookup_name,
                    "testthat",
                    scope,
                ),
                crate::resolve::SpecialCallProvenance::Proven
            )
        {
            return;
        }
        let Some((object, expected)) = testthat_compared_operands(args) else {
            return;
        };
        if self.deterministic_operand(&object.value, scope)
            && self.deterministic_operand(&expected.value, scope)
            && exprs_equivalent(&object.value, &expected.value)
        {
            self.emit(
                Severity::Warning,
                expected.span,
                "RY122",
                format!(
                    "both operands of `{lookup_name}()` are the same deterministic expression; the assertion does not check two different values"
                ),
            );
        }
    }

    /// RY125: a proven base `ncol()`/`nrow()`/`dim()` whose argument is
    /// a definite function value always returns `NULL` (the broom
    /// `ncol(data)` shape, where `data` resolves to `utils::data`).
    fn check_dimensional_on_function(
        &mut self,
        name: &str,
        func: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &Scope,
    ) {
        if !crate::semantic_lists::is_dimensional_function(lookup_name)
            || !matches!(
                self.special_call_provenance(name, func, semantic_name, lookup_name, "base", scope),
                crate::resolve::SpecialCallProvenance::Proven
            )
        {
            return;
        }
        if let Some(arg) = dimensional_argument(args)
            && let Some(subject) = self.definite_function_value(&arg.value, scope)
        {
            self.emit(
                Severity::Warning,
                arg.span,
                "RY125",
                format!(
                    "`{lookup_name}()` of {subject} is always `NULL`; a function has no dimensions"
                ),
            );
        }
    }

    /// RY125 in cli templates (#570): literal template arguments to a
    /// proven cli message function have their `{...}` expression
    /// regions checked for the dimensional-call-on-function shape in
    /// the caller's lexical scope. `.envir`, masked callees, and
    /// dynamic (non-literal) strings stay silent; free names are
    /// never reported on their own.
    fn check_cli_template(
        &mut self,
        name: &str,
        func: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &Scope,
    ) {
        if !crate::semantic_lists::is_cli_message_function(lookup_name)
            || args.iter().any(|arg| arg.name.as_deref() == Some(".envir"))
            || !matches!(
                self.special_call_provenance(name, func, semantic_name, lookup_name, "cli", scope),
                crate::resolve::SpecialCallProvenance::Proven
            )
        {
            return;
        }
        let mut literals: Vec<(&str, Span)> = Vec::new();
        for arg in args {
            // Control arguments (`.frame`, `call`, ...) are not templates.
            if arg
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with('.') || matches!(n, "call" | "class"))
            {
                continue;
            }
            self.collect_template_literals(&arg.value, scope, &mut literals);
        }
        let mut hits: Vec<(String, String, Span)> = Vec::new();
        let mut parser: Option<ry_core::RParser> = None;
        // cli evaluates a template's regions sequentially in one
        // environment: once a region rebinds a name, later regions no
        // longer resolve against the caller's scope and stay silent.
        let mut shadowed = false;
        for (text, span) in literals {
            if shadowed {
                break;
            }
            for (start, end) in cli_template_regions(text) {
                // Cheap gate: regions without a dimensional callee name
                // cannot contain the RY125 shape and are never parsed.
                let region = &text[start..end];
                if !["ncol", "nrow", "dim"].iter().any(|n| region.contains(n)) {
                    // A non-candidate region can still rebind a name a
                    // later region resolves; without parsing it we can
                    // only see literal assignment operators.
                    if region.contains("<-") || region.contains("<<-") {
                        shadowed = true;
                        break;
                    }
                    continue;
                }
                if parser.is_none() {
                    parser = ry_core::RParser::new().ok();
                }
                let Some(parser) = parser.as_mut() else {
                    return;
                };
                let region_span = self.template_region_span(text, span, start, end);
                if self.check_template_region(parser, region, scope, region_span, &mut hits) {
                    shadowed = true;
                    break;
                }
            }
        }
        for (callee, subject, span) in hits {
            self.emit(
                Severity::Warning,
                span,
                "RY125",
                format!("`{callee}()` of {subject} is always `NULL`; a function has no dimensions"),
            );
        }
    }

    /// Literal strings that reach a cli template slot: a bare string,
    /// or the elements of a proven-base `c(...)` bullet vector.
    fn collect_template_literals<'a>(
        &self,
        expr: &'a Expr,
        scope: &Scope,
        out: &mut Vec<(&'a str, Span)>,
    ) {
        match expr {
            Expr::String(text, span) => out.push((text, *span)),
            Expr::Call { func, args, .. } => {
                let Some(callee) = callee_name(func) else {
                    return;
                };
                let semantic: &str = scope.function_alias(&callee).unwrap_or(&callee);
                let bare = crate::semantic_lists::bare_name(semantic);
                if bare == "c"
                    && matches!(
                        self.special_call_provenance(&callee, func, semantic, bare, "base", scope),
                        crate::resolve::SpecialCallProvenance::Proven
                    )
                {
                    for arg in args {
                        self.collect_template_literals(&arg.value, scope, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// Source span of one interpolation region (`{` through `}`)
    /// inside a template literal. Byte offsets in the decoded text
    /// map to source offsets only when the literal contains no escape
    /// and is not a raw string; otherwise the whole literal's span is
    /// kept rather than a wrong narrower one.
    fn template_region_span(&self, text: &str, literal: Span, start: usize, end: usize) -> Span {
        let Some(source) = self.source_text(literal) else {
            return literal;
        };
        if source.contains('\\') || !(source.starts_with('"') || source.starts_with('\'')) {
            return literal;
        }
        let before = &text[..start];
        let newlines = before.matches('\n').count();
        let (line, col) = if newlines == 0 {
            (literal.line, literal.col + 1 + start)
        } else {
            (
                literal.line + newlines,
                start - before.rfind('\n').map_or(0, |pos| pos + 1),
            )
        };
        Span::new(literal.start + start, literal.start + end + 2, line, col)
    }

    /// Parse one template region as R code and record every
    /// dimensional call on a definite function it contains. Regions
    /// that do not parse, or that rebind names (assignment, `for`),
    /// are skipped: template fragments we cannot model stay silent.
    /// Returns whether the region rebinds a name, so the caller can
    /// stop checking later regions of the same template.
    fn check_template_region(
        &self,
        parser: &mut ry_core::RParser,
        region: &str,
        scope: &Scope,
        span: Span,
        hits: &mut Vec<(String, String, Span)>,
    ) -> bool {
        let Ok(file) = parser.parse("<cli-template>", region) else {
            return false;
        };
        if !file.parse_errors.is_empty() {
            return false;
        }
        let mut rebinds = false;
        let mut found: Vec<(String, String, Span)> = Vec::new();
        let policy = Walk {
            assign_targets: false,
            assign_operands: false,
            dollar_args: false,
            fn_bodies: false,
            control_tests: true,
        };
        let _ = ry_core::walk::walk_stmts::<()>(&file.stmts, policy, |node, _| {
            match node {
                AstNode::Stmt(Stmt::Assign { .. } | Stmt::For { .. }) => rebinds = true,
                AstNode::Expr(Expr::BinOp {
                    op: BinOpKind::Assign | BinOpKind::SuperAssign,
                    ..
                }) => rebinds = true,
                AstNode::Expr(Expr::Call { func, args, .. }) => {
                    // Quoted code never runs here, and data-mask calls
                    // resolve names against their mask, not this
                    // scope: neither subtree is checked.
                    if let Expr::Ident { name, .. } = func.as_ref()
                        && matches!(
                            crate::semantic_lists::bare_name(semantic_argument_name(name)),
                            "quote"
                                | "substitute"
                                | "expression"
                                | "with"
                                | "within"
                                | "transform"
                                | "subset"
                                | "eval"
                                | "evalq"
                        )
                    {
                        return ControlFlow::Continue(Descend::Skip);
                    }
                    if let Some((callee, subject)) =
                        self.dimensional_function_call(func, args, scope)
                    {
                        found.push((callee, subject, span));
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        });
        if !rebinds {
            hits.extend(found);
        }
        rebinds
    }

    /// The (callee, subject) pair when `func(args)` is a proven-base
    /// dimensional call on a definite function value.
    fn dimensional_function_call(
        &self,
        func: &Expr,
        args: &[Arg],
        scope: &Scope,
    ) -> Option<(String, String)> {
        let name = callee_name(func)?;
        let semantic = scope.function_alias(&name).unwrap_or(&name).to_string();
        let lookup = crate::semantic_lists::bare_name(&semantic).to_string();
        if !crate::semantic_lists::is_dimensional_function(&lookup)
            || !matches!(
                self.special_call_provenance(&name, func, &semantic, &lookup, "base", scope),
                crate::resolve::SpecialCallProvenance::Proven
            )
        {
            return None;
        }
        let arg = dimensional_argument(args)?;
        let subject = self.definite_function_value(&arg.value, scope)?;
        Some((lookup, subject))
    }

    /// A description of `expr` when it is provably a plain function
    /// value: a function literal, a lexical binding typed as an
    /// unclassed function, or a name that resolves only to a known
    /// package/project function. Project variables, external
    /// bindings, data-mask columns, and classed values (which could
    /// carry a `dim` method) are all excluded.
    fn definite_function_value(&self, expr: &Expr, scope: &Scope) -> Option<String> {
        if scope.data_mask_unknown || scope.data_mask_columns_first() {
            return None;
        }
        match expr {
            Expr::Function { .. } => Some("a function literal".to_string()),
            Expr::Ident { name, .. } => {
                if let Some((pkg, member)) = qualified_parts(name) {
                    let known = if crate::resolve::BASE_DATABASE_PACKAGES.contains(&pkg) {
                        self.typeshed.functions.contains_key(member)
                    } else {
                        self.package_typeshed(pkg)
                            .is_some_and(|t| t.functions.contains_key(member))
                    };
                    return known.then(|| format!("the function `{name}`"));
                }
                if let Some(ty) = scope.get(name) {
                    // Only a known-empty class proves no `dim` method
                    // can dispatch: an unknown class (e.g. from
                    // `structure(f, class = cl)`) may carry one.
                    let plain = ty.mode == Mode::Function && ty.class.known && ty.class.len == 0;
                    return plain.then(|| format!("the function `{name}`"));
                }
                if scope.search_path_unknown
                    || self.external_bindings.contains(name)
                    || self.known_vars.contains(name)
                {
                    return None;
                }
                if let Some(value) = self.resolve_typeshed_value(name) {
                    return (value.mode == Mode::Function)
                        .then(|| format!("the function `{name}`"));
                }
                let known = self.typeshed.functions.contains_key(name)
                    || self.fn_table.functions.binds(name)
                    || self.bare_loaded.iter().any(|pkg| {
                        self.package_is_known(pkg)
                            && self
                                .package_typeshed(pkg)
                                .is_some_and(|t| t.functions.contains_key(name))
                    })
                    || self
                        .typeshed
                        .globals
                        .ambient_functions
                        .iter()
                        .any(|f| f == name);
                known.then(|| format!("the function `{name}`"))
            }
            _ => None,
        }
    }

    /// Whether repeated evaluation of `expr` is provably free of side
    /// effects and ambient state, within RY122's narrow set.
    fn deterministic_operand(&self, expr: &Expr, scope: &Scope) -> bool {
        match expr {
            // `NA`/`NaN` constants are excluded: asserting a value is NA
            // by comparing constants is a deliberate idiom, not the
            // duplicated-expression typo this rule targets.
            Expr::Logical(..) | Expr::Integer(..) | Expr::String(..) | Expr::Null(..) => true,
            Expr::Double(value, _) => value.is_finite(),
            Expr::Ident { name, .. } => scope.get(name).is_some_and(|ty| {
                !matches!(ty.mode, Mode::Function | Mode::Environment | Mode::Language)
            }),
            Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            } => {
                // Rebound `-` is excluded by `operator_rebound`; limiting
                // the operand to finite numeric literals is deliberate
                // extra conservatism (a non-literal `-x` stays silent).
                (matches!(expr.as_ref(), Expr::Integer(..))
                    || matches!(expr.as_ref(), Expr::Double(v, _) if v.is_finite()))
                    && !ops_chooser::operator_rebound(self, "-", scope)
            }
            Expr::Index {
                base, kind, args, ..
            } => {
                let Expr::Ident { name, .. } = base.as_ref() else {
                    return false;
                };
                match kind {
                    // The parser records the `$` field as the argument
                    // name, not as an evaluated expression.
                    IndexKind::Dollar => {
                        self.plain_list_receiver(name, scope)
                            && args.len() == 1
                            && args[0].name.is_some()
                    }
                    IndexKind::Single | IndexKind::Double => {
                        self.plain_index_receiver(name, scope)
                            && args.len() == 1
                            && args[0].name.is_none()
                            && matches!(
                                args[0].value,
                                Expr::String(..) | Expr::Integer(..) | Expr::Double(..)
                            )
                    }
                    IndexKind::Slot => false,
                }
            }
            Expr::Call { func, args, .. } => {
                let Some(name) = ident_name(func) else {
                    return false;
                };
                // Lenient (no search-path guard), like RY100's math-fn
                // recognition: package test files run under an attached
                // search path ry cannot prove, and the strict variant
                // would silence the rule exactly where it applies. A
                // lexical or project redefinition still stays silent.
                crate::semantic_lists::BASE_MATH_FNS
                    .contains(&crate::semantic_lists::bare_name(name))
                    && self.resolves_to_base_lenient(name, scope)
                    && args
                        .iter()
                        .all(|arg| self.deterministic_operand(&arg.value, scope))
                    && !args
                        .iter()
                        .any(|arg| reads_known_classed_ident(&arg.value, scope))
            }
            _ => false,
        }
    }

    /// A `$` receiver proven to be a plain (unclassed) list, so `$`
    /// is base list access rather than a possible `$.foo` method.
    /// Both a proven class and an unknown one (`structure(list(...),
    /// class = cl)`) refuse: unlike a bare binding read, `$` on an
    /// unknown class can dispatch.
    fn plain_list_receiver(&self, name: &str, scope: &Scope) -> bool {
        scope.get(name).is_some_and(|ty| {
            ty.class.known
                && ty.class.len == 0
                && (matches!(ty.mode, Mode::List) || scope.has_list_origin(name))
        })
    }

    /// A `[`/`[[` receiver proven to be plain data: an unclassed list
    /// or atomic vector, where base subsetting applies.
    fn plain_index_receiver(&self, name: &str, scope: &Scope) -> bool {
        self.plain_list_receiver(name, scope)
            || scope.get(name).is_some_and(|ty| {
                ty.class.known
                    && ty.class.len == 0
                    && matches!(
                        ty.mode,
                        Mode::Logical
                            | Mode::Integer
                            | Mode::Double
                            | Mode::Complex
                            | Mode::Character
                            | Mode::Raw
                    )
            })
    }

    /// `hasArg` (captures its argument name) and `on.exit` (evaluates
    /// `expr` when the enclosing function returns).
    fn infer_deferred_call(
        &mut self,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
        proven: bool,
    ) -> Option<RType> {
        // Model the quoting of `hasArg` so a non-formal does not also
        // produce RY010. With `...` in the formals, `hasArg(name)`
        // legitimately matches dots-supplied arguments (the
        // `if (hasArg(b)) list(...)$b` idiom), so only a function without
        // `...` makes the check provably FALSE.
        if lookup_name == "hasArg" {
            if let Some(name) = args.first().and_then(|argument| match &argument.value {
                Expr::Ident { name, .. } | Expr::String(name, _) => Some(name),
                _ => None,
            }) && proven
                && let Some(formals) = self.enclosing_formals.last()
                && !formals.has_dots
                && !formals.names.contains(name)
            {
                self.emit(
                    Severity::Warning,
                    span,
                    "RY096",
                    format!(
                        "`hasArg({name})` names a parameter that is not a formal; it is always FALSE"
                    ),
                );
            }
            return Some(RType::scalar(Mode::Logical));
        }

        // Names assigned later in the enclosing body exist by the time the
        // `on.exit` expression runs. Seed only those statically assigned
        // names and still infer the expression normally, so genuinely
        // unbound names retain RY010.
        if lookup_name == "on.exit" {
            let expression_index = args
                .iter()
                .position(|argument| argument.name.as_deref() == Some("expr"))
                .or_else(|| args.iter().position(|argument| argument.name.is_none()));
            for (index, argument) in args.iter().enumerate() {
                if Some(index) == expression_index {
                    let mut exit_scope = scope.independent_execution_scope();
                    // The exit code runs after the rest of the body, which
                    // is not walked yet, so it neither keeps nor proves a
                    // scalar fact.
                    exit_scope.invalidate_scalar_assertions();
                    if let Some(assigned) = self.deferred_captures.last() {
                        for name in assigned {
                            if exit_scope.get(name).is_none() {
                                exit_scope.insert(name.clone(), RType::unknown());
                            }
                        }
                    }
                    self.infer(&argument.value, &mut exit_scope);
                } else {
                    self.infer(&argument.value, scope);
                }
            }
            return Some(RType::new(Mode::Null, Length::Zero));
        }
        None
    }

    /// RY094: `sprintf` / `gettextf` with a literal format string that
    /// requires more value arguments than the call supplies.
    fn check_printf_format_arity(&mut self, lookup_name: &str, args: &[Arg]) {
        if matches!(lookup_name, "sprintf" | "gettextf")
            && let Some(Expr::String(format, format_span)) = args.first().map(|arg| &arg.value)
            && let Some(required) = printf_argument_count(format)
            && let supplied = args
                .iter()
                .skip(1)
                .filter(|arg| !matches!(arg.value, Expr::Missing(_)))
                .count()
            && supplied < required
        {
            self.emit(
                Severity::Warning,
                *format_span,
                "RY094",
                format!(
                    "format string requires {required} value argument(s), but {} provided",
                    supplied
                ),
            );
        }
    }

    /// The NSE/quoting cluster: calls whose arguments are not regular
    /// evaluated values — package attachment, quoting forms, environment
    /// loaders, FFI primitives, and NSE-symbol functions without stub
    /// eval metadata.
    fn infer_nse_quoting_call(
        &mut self,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> Option<RType> {
        // Detaching a package invalidates the ordinary default-search-path
        // assumption, including methods::new being available as bare `new`.
        if lookup_name == "detach" && self.resolves_to_base(semantic_name, scope) {
            scope.mark_search_path_unknown();
        }

        // `library(foo)` and `require(foo)` take a package name as a bare
        // symbol, not an expression. Inferring their args would trigger
        // spurious RY010 on every `library(magrittr)` etc. We ALSO record
        // the package, and any packages it attaches in turn, into
        // `self.loaded` so the dplyr NSE gating sees them after either call.
        if semantic_name == "library" || semantic_name == "require" {
            if let Some(pkg) = crate::attached_package_name(args) {
                let attached: Vec<String> = ry_workspace::attaching(pkg, &self.user_stubs)
                    .map(str::to_owned)
                    .collect();
                for package in attached {
                    // An attached package without a stub can contribute any
                    // export or lazy-loaded dataset to the search path.
                    if !self.package_is_known(&package) {
                        scope.mark_search_path_unknown();
                    }
                    Arc::make_mut(&mut self.loaded).insert(package.clone());
                    Arc::make_mut(&mut self.bare_loaded).insert(package);
                }
            } else if crate::character_only(args) {
                // `library(pkg, character.only = TRUE)` evaluates its
                // argument. Without a literal package name we cannot
                // know which bindings were attached.
                scope.mark_search_path_unknown();
            }
            return Some(if semantic_name == "require" {
                RType::new(Mode::Logical, Length::One)
            } else {
                RType::new(Mode::Null, Length::Zero)
            });
        }

        // Formula construction and expression-vector constructors quote
        // their language arguments. Names inside them are resolved later in
        // a model/data environment, not at construction time.
        if crate::semantic_lists::is_quoting_form(lookup_name) {
            return Some(RType::unknown());
        }

        // `data(name)` loads one or more datasets into the current
        // environment. Bare names and string literals are data identifiers,
        // not reads of existing variables, and become bindings for following
        // statements. Package/control arguments are not introduced. A local
        // `data` (Shiny's `data <- reactive(...)`) or `load` is an ordinary
        // call instead (#717).
        if semantic_name == "data" && !self.call_head_bound_locally(semantic_name, scope) {
            scope.mark_search_path_unknown();
            for argument in args {
                if argument.name.is_some() {
                    let _ = self.infer(&argument.value, scope);
                    continue;
                }
                let dataset = match &argument.value {
                    Expr::Ident { name, .. } | Expr::String(name, _) => Some(name.clone()),
                    _ => None,
                };
                if let Some(dataset) = dataset {
                    scope.insert(dataset, RType::unknown());
                } else {
                    let _ = self.infer(&argument.value, scope);
                }
            }
            return Some(RType::new(Mode::Character, Length::Unknown));
        }

        if semantic_name == "load" && !self.call_head_bound_locally(semantic_name, scope) {
            scope.mark_search_path_unknown();
            self.infer_args_for_diagnostics(args, scope);
            if let Some(bindings) = self.load_bindings.get(&span.start).cloned() {
                for binding in bindings {
                    if binding == ry_core::SERIALIZED_BINDINGS_UNENUMERABLE {
                        // An unenumerable workspace may introduce any
                        // binding, so open the search path instead of
                        // enumerating names.
                        scope.mark_search_path_unknown();
                    } else {
                        scope.insert(binding, RType::unknown());
                    }
                }
            }
            return Some(RType::new(Mode::Character, Length::Unknown));
        }

        // `requireNamespace("pkg")` makes qualified `pkg::name` lookups
        // available, but unlike library/require it does NOT attach the
        // package or introduce unqualified bindings. Let it fall through
        // to the base typeshed without adding it to `self.loaded`.

        // Foreign-function-interface primitives (`.Call`, `.C`,
        // `.Fortran`, `.External`, `.External2`, `.Internal`). Their
        // FIRST argument is a C/Fortran entry-point symbol, conventionally
        // written as a bare identifier or backtick symbol (e.g.
        // `.Call(glue_, x)`), NOT a variable reference. Inferring it
        // normally would fire a spurious RY010. Skip RY010 on a
        // bare-symbol first arg, infer the remaining args normally, and
        // return opaque (the return type depends on the native routine).
        //
        // Wrappers that forward to a primitive (`call_with_cleanup`) follow
        // the same convention, but they are ordinary redefinable R
        // functions, so they only get this treatment in a package that
        // declares `useDynLib(..., .registration = TRUE)`.
        if ry_core::FFI_PRIMITIVES.contains(&semantic_name)
            || (is_registered_ffi_wrapper(semantic_name) && self.package_facts.native_registration)
        {
            for (i, a) in args.iter().enumerate() {
                if i == 0 {
                    // The entry-point symbol: a bare identifier or
                    // backtick-quoted name is not a variable read.
                    let is_symbol = matches!(&a.value, Expr::Ident { .. });
                    if is_symbol {
                        continue;
                    }
                }
                let _ = self.infer(&a.value, scope);
            }
            return Some(RType::unknown());
        }

        // NSE-symbol functions without stub eval metadata: take bare
        // symbol arguments that should NOT be resolved as variable
        // references. We return opaque without evaluating the args as
        // expressions, suppressing spurious RY010. Functions whose
        // stubs declare eval modes are NOT listed here; the per-signature
        // EvalMode loop in the argument-inference stage handles them
        // (see `is_nse_symbol_fn`).
        if is_nse_symbol_fn(lookup_name) {
            return Some(RType::unknown());
        }
        None
    }

    /// The default two-argument `assign("name", value)`: rebind in the
    /// current environment.
    fn infer_local_assign_call(
        &mut self,
        semantic_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        if semantic_name != "assign" || args.len() != 2 {
            return None;
        }
        let name = match &args[0].value {
            Expr::String(name, _) => Some(name.clone()),
            _ => None,
        };
        let _ = self.infer(&args[0].value, scope);
        let value = self.infer(&args[1].value, scope);
        if let Some(name) = name {
            scope.insert(name, value.clone());
        }
        Some(value)
    }

    /// Base `delayedAssign` binds an unforced promise in this frame, so the
    /// target becomes an unknown-typed binding and `value` is never inferred.
    fn infer_local_delayed_assign_call(
        &mut self,
        semantic_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        if crate::semantic_lists::bare_name(semantic_name) != "delayedAssign"
            || !self.resolves_to_base_lenient(semantic_name, scope)
        {
            return None;
        }
        // R rejects unmatched or repeated arguments; `assign.env` binds elsewhere.
        let bindings = match_arguments(&["x", "value", "eval.env", "assign.env"], args);
        if !bindings.is_one_to_one()
            || bindings.arg_for_param(1).is_none()
            || bindings.arg_for_param(3).is_some()
        {
            return None;
        }
        let target = &args[bindings.arg_for_param(0)?];
        let _ = self.infer(&target.value, scope);
        if let Some(environment) = bindings.arg_for_param(2) {
            let _ = self.infer(&args[environment].value, scope);
        }
        if let Expr::String(name, _) = &target.value {
            scope.insert(name.clone(), RType::unknown());
        }
        Some(RType::new(Mode::Null, Length::Zero))
    }

    /// The argument-inference stage: resolves the call's signatures
    /// (typeshed, inherited S3, project FnTable) and infers each argument's
    /// type, honoring the stub-declared per-parameter evaluation modes.
    fn infer_argument_types(
        &mut self,
        name: &str,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
        call_span: Span,
    ) -> CallResolution {
        let head = self.resolve_call_head(name, semantic_name, lookup_name, args, scope, call_span);
        let resolved_sig = head.signature();
        let user_function = head.user_function();
        if let Some(function) = user_function {
            self.record_signature_read(function.return_slot);
        }
        let supplied_data_mask_source = resolved_sig.and_then(|signature| {
            data_mask_source_arg(signature, args).map(|argument_index| {
                (
                    argument_index,
                    self.infer(&args[argument_index].value, scope),
                )
            })
        });
        let user_argument_matches = user_function.map(|function| {
            let names: Vec<&str> = function
                .params
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect();
            match_arguments(&names, args)
        });
        let unforced_args = &head.unforced_args;
        // One match for the resolved signature; the loop below used to redo it
        // for every argument.
        let declared_binding = resolved_sig
            .as_ref()
            .map(|signature| (signature, match_params(&signature.params, args)));
        let mut arg_types: Vec<RType> = Vec::with_capacity(args.len());
        for (index, a) in args.iter().enumerate() {
            let declared_mode = declared_binding
                .as_ref()
                .and_then(|(signature, bindings)| eval_mode_for_arg(signature, bindings, index));
            let user_dispatch = head.inherited_s3_metadata
                || user_function.is_some()
                || supplied_data_mask_source
                    .as_ref()
                    .map(|(_, data)| data)
                    .or_else(|| arg_types.first())
                    .is_some_and(|first| self.resolves_user_s3_dispatch(lookup_name, first));
            // The user formal this actual bound to (directly or through
            // `...`), looked up once for both the defusing and quoting
            // flags below.
            let user_param = user_argument_matches
                .as_ref()
                .and_then(|matches| matches.param_for_arg[index].or(matches.dots))
                .and_then(|parameter| user_function?.params.get(parameter));
            let injection = resolved_sig
                .as_ref()
                .and_then(|sig| argument_supports_injection(sig, args, index))
                .max(user_param.and_then(|parameter| parameter.injection))
                // An unresolved callable may capture its arguments. Absence of
                // metadata cannot prove that repeated negation is evaluated.
                .or_else(|| {
                    (resolved_sig.is_none() && user_function.is_none())
                        .then_some(InjectionMode::Full)
                });
            // Dynamic dots allow name injection on the left of `:=`, even
            // when value arguments accept splicing only.
            let injection = if lookup_name == ":=" && index == 0 && scope.tidy_injection.is_some() {
                Some(InjectionMode::Full)
            } else {
                injection
            };
            if unforced_args.get(index).copied().unwrap_or(false) {
                // R never evaluates this actual, so it gets no diagnostics
                // and no effects on the caller. Its type still serves
                // declared-contract checks.
                let mut unevaluated = scope.independent_execution_scope();
                arg_types.push(self.infer_discarding(&a.value, &mut unevaluated));
                continue;
            }
            if let Some(parameter) = user_param
                && (parameter.quoting
                    || declared_mode.is_none_or(|mode| matches!(mode, EvalMode::Normal)))
                && let forced = user_function
                    .zip(user_argument_matches.as_ref())
                    .is_some_and(|(function, matches)| {
                        omits_forcing_default(&function.params, matches, args, parameter)
                    })
                && let Some(captured) =
                    self.infer_user_param_arg(parameter, forced, &a.value, scope)
            {
                arg_types.push(captured);
                continue;
            }
            if let Some((_, data)) = supplied_data_mask_source
                .as_ref()
                .filter(|(source_index, _)| *source_index == index)
            {
                arg_types.push(data.clone());
                continue;
            }
            if let Some(mode) = declared_mode {
                let inferred = match mode {
                    EvalMode::Normal => self.infer_with_injection(&a.value, scope, injection),
                    EvalMode::QuotedSymbol => {
                        if matches!(a.value, Expr::Ident { .. }) {
                            RType::unknown()
                        } else {
                            self.infer_discarding(&a.value, scope)
                        }
                    }
                    EvalMode::QuotedExpression | EvalMode::CapturesPromise => RType::unknown(),
                    EvalMode::DataMask => {
                        // Formula extras evaluate in the caller when their
                        // declared data argument is absent.
                        let Some(data) = supplied_data_mask_source
                            .as_ref()
                            .map(|(_, data)| data.clone())
                            .or_else(|| {
                                resolved_sig
                                    .as_ref()
                                    .is_some_and(|signature| signature.data_mask_source.is_none())
                                    .then(RType::unknown)
                            })
                        else {
                            arg_types.push(self.infer_with_injection(&a.value, scope, injection));
                            continue;
                        };
                        let mut local = self.dplyr_data_mask_scope(scope, &data);
                        local.insert(".", RType::unknown());
                        if user_dispatch {
                            local = local.with_unknown_data_mask();
                        }
                        self.infer_with_injection(&a.value, &mut local, injection)
                    }
                    EvalMode::TidySelect => {
                        let data = supplied_data_mask_source
                            .as_ref()
                            .map(|(_, data)| data.clone())
                            .unwrap_or_else(RType::unknown);
                        let mut local = self.dplyr_data_mask_scope(scope, &data);
                        if user_dispatch {
                            local = local.with_unknown_data_mask();
                        }
                        self.infer_tidyselect_expr(&a.value, &mut local, injection)
                    }
                };
                arg_types.push(inferred);
            } else {
                arg_types.push(self.infer_with_injection(&a.value, scope, injection));
            }
        }
        CallResolution {
            arg_types,
            head,
            locally_shadows_stub: false,
        }
    }

    /// The dynamic-loader stage: scope-populating calls suppress
    /// unbound-name diagnostics only in the lexical scope they can
    /// actually populate. Records `locally_shadows_stub` on the
    /// resolution for the stub-assertion stage below.
    fn note_dynamic_loader_scope(
        &self,
        name: &str,
        resolution: &mut CallResolution,
        args: &[Arg],
        scope: &mut Scope,
    ) {
        resolution.locally_shadows_stub = !name.contains("::")
            && scope.get(name).is_some()
            && scope.function_alias(name).is_none();
        if !resolution.locally_shadows_stub
            && (name.contains("::") || resolution.head.user_function().is_none())
            && resolution.head.signature().is_some_and(|signature| {
                scope_effect_populates_current_scope(
                    signature,
                    args,
                    self.enclosing_formals.is_empty(),
                )
            })
        {
            scope.mark_search_path_unknown();
        }
    }

    /// The hardcoded `assert_*_scalar` narrowing stage. The stub-assertion
    /// stage below reads the same knowledge from a signature's `assertion`
    /// field. The two sites are duplicates by necessity: no stub declares
    /// the `assert_*_scalar` helpers yet. Folding them into the stubs is
    /// blocked on r-typeshed (issue #41); see `assertion_call_target`.
    fn infer_assert_scalar_call(
        &mut self,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        let target = assertion_call_target(lookup_name)?;
        if let Some(Expr::Ident { name: var, .. }) = args.first().map(|a| &a.value) {
            scope.insert(var.clone(), target);
        }
        Some(RType::new(Mode::Null, Length::Zero))
    }

    /// The stub-driven assertion stage: a signature declaring an `assertion`
    /// narrows its subject binding, or proves the call unreachable with
    /// RY092 when the actual type is provably rejected. rlang's standalone
    /// helpers are package-local, so package sources need not have a literal
    /// `library(rlang)` call; a local function must carry the same
    /// fingerprint.
    fn infer_stub_assertion_call(
        &mut self,
        name: &str,
        lookup_name: &str,
        resolution: &CallResolution,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        let locally_shadows_stub = resolution.locally_shadows_stub;
        let assertion_signature = resolution.head.signature().or_else(|| {
            (!name.contains("::")
                && (!locally_shadows_stub || resolution.head.user_function().is_some()))
            .then(|| ry_typeshed::load_package("rlang"))
            .flatten()
            .and_then(|typeshed| typeshed.functions.get(lookup_name))
        });
        // The assertion path consults the argument match three times. Build it
        // once, and only for signatures that actually declare an assertion.
        let assertion_binding = assertion_signature
            .filter(|signature| signature.assertion.is_some())
            .map(|signature| (signature, match_params(&signature.params, args)));
        if (name.contains("::")
            || !locally_shadows_stub
            || resolution.head.user_function().is_some())
            && let Some((signature, bindings)) = assertion_binding
            && let Some(assertion) = signature.assertion.as_ref()
            && assertion_is_provenanced(signature, assertion)
            && resolution.head.user_function().is_none_or(|function| {
                assertion
                    .provenance
                    .fingerprint_params
                    .iter()
                    .all(|fingerprint| {
                        function
                            .params
                            .iter()
                            .any(|param| param.name == *fingerprint)
                    })
            })
            && let Some(subject_index) =
                bound_argument_index_matched(&signature.params, &bindings, &assertion.subject_param)
            && let Some(Expr::Ident { name: var, .. }) =
                args.get(subject_index).map(|arg| &arg.value)
        {
            let mut target = guard_target(json_rtype_to_rtype(&assertion.target));
            // Non-literal opt-ins are conservatively treated like TRUE.
            for (param, null_target) in [
                (
                    assertion.allow_null_param.as_deref(),
                    Some(RType::new(Mode::Null, Length::Zero)),
                ),
                (
                    assertion.allow_na_param.as_deref(),
                    Some(RType::scalar(Mode::Logical)),
                ),
            ] {
                if let (Some(param), Some(weakening)) = (param, null_target)
                    && bound_argument_index_matched(&signature.params, &bindings, param)
                        .is_some_and(|index| !matches!(args[index].value, Expr::Logical(false, _)))
                {
                    target = target.join(weakening);
                }
            }
            let actual = &resolution.arg_types[subject_index];
            if !scope.is_default_parameter(var)
                && standalone_check_provably_rejects(actual, &target)
            {
                self.emit(
                    Severity::Error,
                    args[subject_index].span,
                    "RY092",
                    format!(
                        "argument `{var}` to `{lookup_name}` is `{actual}`, expected {}",
                        expected_type_label(&target)
                    ),
                );
                scope.unreachable = true;
            } else {
                scope.insert(var.clone(), target);
            }
            return Some(RType::new(Mode::Null, Length::Zero));
        }
        None
    }

    /// The assertion-predicate stage: `stopifnot(...)` and `assert_that(...)`
    /// narrow the enclosing scope with each predicate's positive-path fact.
    fn apply_assertion_predicates(&mut self, name: &str, args: &[Arg], scope: &mut Scope) {
        let assertion_predicates =
            name == "stopifnot" || name == "assert_that" || name == "assertthat::assert_that";
        if assertion_predicates {
            for (index, argument) in args.iter().enumerate() {
                // `msg` and stopifnot's `local`/`exprs`/`exprObject` are
                // controls, not predicates.
                if (name.ends_with("assert_that") && argument.name.as_deref() == Some("msg"))
                    || (name == "stopifnot" && !stopifnot_predicate_arg(argument))
                {
                    continue;
                }
                // Arguments run left to right. A later argument that
                // assigns a name voids an earlier predicate's fact about it.
                let mut later_assigned = HashSet::new();
                for later in &args[index + 1..] {
                    binop::collect_condition_assignment_names(&later.value, &mut later_assigned);
                }
                let narrowing = self.extract_branch_narrowing(&argument.value, scope);
                if !narrowing.mentions_any(&later_assigned) {
                    apply_narrowing_branch(scope, &narrowing, NarrowingBranch::Then);
                }
            }
            if name == "stopifnot" {
                self.mark_scalar_assertions(args, scope);
            }
        }
    }

    /// `UseMethod` looks up methods in the calling environment before the
    /// generic's: a `<generic>.<class>` binding in scope that is not the
    /// table's definition may be the selected method.
    fn caller_method_may_shadow(&self, generic: &str, receiver: &RType, scope: &Scope) -> bool {
        let classes = receiver
            .class
            .names
            .iter()
            .take(receiver.class.len as usize)
            .flatten();
        for class in classes.map(|class| &**class).chain(["default"]) {
            let method = format!("{generic}.{class}");
            if scope.get(&method).is_some() {
                return !self
                    .fn_table
                    .functions
                    .live_named(&method)
                    .is_some_and(|function| {
                        function.source_path == self.path
                            && scope.lexical_definition(&method) == Some(function.definition_span)
                    });
            }
            if self.s3_lookup_method(generic, class).is_some() {
                return false;
            }
        }
        false
    }

    /// The lexical-callable stage: a `Function`-typed scope binding with an
    /// inferred `fn_sig` resolves to the signature's return type (this is
    /// what makes `c <- make_counter(); v <- c()` work). Qualified calls
    /// bypass local bindings: `pkg::f()` selects `f` from `pkg`.
    fn infer_lexical_callable_call(
        &mut self,
        name: &str,
        args: &[Arg],
        arg_types: &[RType],
        span: Span,
        head: &CallHead,
    ) -> Option<RType> {
        if name.contains("::") {
            return None;
        }
        let t = head.lexical_binding.as_ref()?;
        if matches!(t.mode, Mode::Function) {
            if let Some(sig) = &t.fn_sig {
                return Some((*sig.return_type).clone());
            }
            // Bound function value without an inferred signature:
            // opaque. We do NOT fall through to the FnTable path,
            // because a scope-local binding shadows top-level
            // definitions and we have no way to refine the local
            // one. Returning opaque here is the conservative
            // choice (no false positives, possible false negatives).
            return Some(RType::unknown());
        }
        if let Some(result) = self.callable_function_union(t, args, arg_types) {
            return Some(result);
        }
        if head.opaque_callable && head.user_function().is_none() {
            // A binding that may be a function stops R's call lookup, so
            // no same-named stub applies (#722).
            return Some(RType::unknown());
        }
        if !t.mode.is_unknown() {
            // R's function/value namespace separation: when a name is
            // CALLED, R searches the environment chain for a *function*
            // named `name` and skips non-function bindings. So a local
            // non-function binding (e.g. `lengths <- lengths(x)`) does
            // NOT shadow a same-named function in the typeshed or
            // FnTable at a call site. If such a function exists, fall
            // through to the resolution below instead of firing RY070.
            // Only when no function of that name exists anywhere does
            // calling the non-function value warrant RY070; an
            // uncertain outward state also falls through in silence
            // (see `call_head_function_evidence`).
            if matches!(head.function_evidence, CallHeadFunctionEvidence::Diagnose) {
                // RY070: no function of this name is reachable in any
                // frame, so R errors at runtime with
                // "could not find function". Args have already been
                // inferred above, so we just emit and return opaque
                // (re-inferring would double-emit arg diagnostics).
                return Some(self.emit_symbol_not_callable(name, t.mode, span));
            }
            // A function exists elsewhere; fall through to resolve it
            // (the local non-function binding is ignored at the call
            // site, matching R).
        }
        // Opaque: fall through; the name might still resolve via
        // the FnTable or typeshed below.
        None
    }

    /// Returns a value for a union call when some member is a closure.
    /// A union that also holds a non-function (NULL-or-function,
    /// value-or-function) is opaque: R's function lookup calls the closure
    /// on its path and skips the other arm, so the call fails only on paths
    /// whose reachability depends on invariants outside the call (#700).
    fn callable_function_union(
        &mut self,
        ty: &RType,
        args: &[Arg],
        arg_types: &[RType],
    ) -> Option<RType> {
        let members = ty.members.as_ref()?;
        if ty.mode != Mode::Union || !members.iter().any(|member| member.mode == Mode::Function) {
            return None;
        }
        if members.iter().any(|member| member.mode != Mode::Function) {
            return Some(RType::unknown());
        }

        let signatures: Vec<_> = members
            .iter()
            .filter_map(|member| member.fn_sig.as_ref())
            .collect();
        for (index, actual) in arg_types.iter().enumerate() {
            let expected: Vec<_> = signatures
                .iter()
                .filter_map(|signature| signature.params.get(index))
                .collect();
            if expected.len() == signatures.len()
                && !expected.is_empty()
                && expected
                    .iter()
                    .all(|expected| types_provably_incompatible(actual, expected))
            {
                self.emit(
                    Severity::Error,
                    args[index].span,
                    "RY092",
                    format!(
                        "argument {} is `{}`, incompatible with every callable union member",
                        index + 1,
                        actual.mode
                    ),
                );
            }
        }

        let returns = members.iter().map(|member| {
            member
                .fn_sig
                .as_ref()
                .map(|signature| (*signature.return_type).clone())
                .unwrap_or_else(RType::unknown)
        });
        Some(join_all(returns))
    }

    /// Emit RY070 for a value-expression head (`pkg::value()`) whose
    /// typeshed type is known to be a non-function value, and return
    /// the opaque result every such site yields. Qualified `::` lookup
    /// fetches the value itself, so R's error here is
    /// "attempt to apply non-function", not a function-mode miss.
    fn emit_not_callable(&mut self, name: &str, mode: Mode, span: Span) -> RType {
        self.emit(
            Severity::Error,
            span,
            "RY070",
            format!("`{}` is `{}`, not a function; cannot call it", name, mode),
        );
        RType::unknown()
    }

    /// Emit RY070 for a bare symbol call head with no reachable
    /// function binding (#381). R's function-mode lookup skips the
    /// non-function bindings, so the reachable runtime error is exactly
    /// R's own: the function is not found. Value-expression heads keep
    /// [`Self::emit_not_callable`].
    fn emit_symbol_not_callable(&mut self, name: &str, mode: Mode, span: Span) -> RType {
        self.emit(
            Severity::Error,
            span,
            "RY070",
            format!("could not find function `{name}` (bound here as `{mode}`)"),
        );
        RType::unknown()
    }

    /// Infer every argument of a call for diagnostics only, discarding the
    /// types. The shared tail of the call paths that reject a call shape
    /// before argument types are needed.
    pub(crate) fn infer_args_for_diagnostics(&mut self, args: &[Arg], scope: &mut Scope) {
        for argument in args {
            let _ = self.infer(&argument.value, scope);
        }
    }

    /// Whether `expr` is a `new(...)` call whose head provably is `methods::new`.
    fn is_proven_new_call(&self, expr: &Expr, scope: &Scope) -> bool {
        let Expr::Call { func, .. } = expr else {
            return false;
        };
        let Some(name) = callee_name(func) else {
            return false;
        };
        let semantic_name = scope.function_alias(&name).unwrap_or(&name);
        crate::semantic_lists::bare_name(semantic_name) == "new"
            && matches!(
                self.special_call_provenance(&name, func, semantic_name, "new", "methods", scope),
                crate::resolve::SpecialCallProvenance::Proven
            )
    }

    /// The names declared in the `public` / `private` / `active` lists of an
    /// `R6Class()` call, paired with the type of their declared initialiser.
    ///
    /// Only a literal `list(...)` can be enumerated; a member list produced by
    /// a helper call contributes nothing, so the injected set stays limited to
    /// names R6 is known to define on the object.
    fn r6_member_bindings(
        &mut self,
        args: &[Arg],
        params: &[&str],
        matches: &ArgumentMatch,
        scope: &Scope,
    ) -> Vec<(String, RType)> {
        // `active` is kept separate: R6 turns each of its members into an
        // active binding, so a sibling method reading the bare name gets the
        // getter's *result*, not the function. Typing it `function` makes
        // `total + n` an RY040 error on correct code.
        let member_lists: Vec<(&Expr, bool)> = args
            .iter()
            .enumerate()
            .filter_map(|(index, argument)| {
                let parameter = matches.param_for_arg[index]
                    .and_then(|index| params.get(index))
                    .copied()?;
                matches!(parameter, "public" | "private" | "active")
                    .then_some((&argument.value, parameter == "active"))
            })
            .collect();
        let only_lists: Vec<&Expr> = member_lists.iter().map(|(list, _)| *list).collect();
        let rebound = r6_rebound_members(&only_lists);
        let mut bindings = Vec::new();
        for (list, is_active) in member_lists {
            let Expr::Call {
                func,
                args: members,
                ..
            } = list
            else {
                continue;
            };
            if ident_name(func) != Some("list") {
                continue;
            }
            for member in members {
                let Some(name) = member.name.as_deref() else {
                    continue;
                };
                let member_type = if rebound.contains(name) || is_active {
                    // An active member's type is whatever its getter returns.
                    // Return-type inference through an R6 active binding is
                    // not modelled, so stay unknown rather than claim it is
                    // the function itself.
                    RType::unknown()
                } else {
                    self.r6_member_type(&member.value, scope)
                };
                bindings.push((name.to_string(), member_type));
            }
        }
        bindings
    }

    /// The type a declared R6 member contributes to sibling method bodies.
    ///
    /// A literal initialiser carries the field's mode (`character(0)` is a
    /// character field). `NULL` is the conventional placeholder for a field
    /// that `initialize()` fills in later, so it declares nothing about the
    /// eventual value and stays unknown.
    fn r6_member_type(&mut self, initialiser: &Expr, scope: &Scope) -> RType {
        match initialiser {
            Expr::Function { .. } => RType::scalar(Mode::Function),
            Expr::Null(_) => RType::unknown(),
            other => {
                // Inferred against a throwaway scope and in discarding mode:
                // the member list is walked again for diagnostics below, and
                // this probe must not emit them twice.
                let mut probe = scope.independent_execution_scope();
                self.infer_discarding(other, &mut probe)
            }
        }
    }

    fn infer_injected_expr(&mut self, expr: &Expr, scope: &mut Scope) -> RType {
        match expr {
            Expr::Function { params, body, .. } => {
                let mut inner = scope.function_execution_scope();
                for parameter in params {
                    inner.insert_parameter(parameter.name.clone(), RType::unknown());
                }
                for name in assigned_names_in_body(body) {
                    inner.insert(name, RType::unknown());
                }
                self.walk_block(body, &mut inner, None);
                RType::scalar(Mode::Function)
            }
            Expr::Call { args, .. } => {
                for argument in args {
                    self.infer_injected_expr(&argument.value, scope);
                }
                RType::unknown()
            }
            Expr::Block { body, .. } => {
                self.walk_block(body, scope, None);
                RType::unknown()
            }
            _ => self.infer(expr, scope),
        }
    }
}

/// Whether an `R6Class()` call opts out of R6's portable evaluation model.
///
/// Only a literal `portable = FALSE` counts. R6's default is portable, and a
/// computed value cannot be resolved here, so anything else keeps the strict
/// reading in which bare member names do not resolve.
fn r6_call_is_non_portable(args: &[Arg], params: &[&str], matches: &ArgumentMatch) -> bool {
    args.iter().enumerate().any(|(index, argument)| {
        matches.param_for_arg[index]
            .and_then(|index| params.get(index))
            .is_some_and(|parameter| *parameter == "portable")
            && matches!(argument.value, Expr::Logical(false, _))
    })
}

/// The member names an R6 class body assigns to after declaring them.
///
/// A declared initialiser only describes the member while the declaration
/// holds. R6 classes routinely declare a placeholder that `initialize()`
/// overwrites with the real object -- shiny's `msg = "<MessageLogger>"` names
/// the eventual class rather than storing a string -- so a member that the
/// body assigns to (`x <<- v`, `self$x <- v`, `private$x <- v`, `x[[i]] <- v`)
/// contributes no type. Over-approximating costs precision, never soundness:
/// the member is still bound, just untyped.
fn r6_rebound_members(member_lists: &[&Expr]) -> HashSet<String> {
    fn record_target(target: &Expr, names: &mut HashSet<String>) {
        match target {
            Expr::Ident { name, .. } => {
                names.insert(name.clone());
            }
            Expr::Index {
                base, kind, args, ..
            } => {
                // `self$x <- v` / `private$x <- v` rebind the member itself;
                // any other subscripted target rebinds its base.
                if *kind == IndexKind::Dollar
                    && matches!(ident_name(base), Some("self" | "private"))
                    && let Some(field) = args.first()
                {
                    match &field.value {
                        Expr::Ident { name, .. } | Expr::String(name, _) => {
                            names.insert(name.clone());
                        }
                        _ => {}
                    }
                }
                record_target(base, names);
            }
            // `class(x) <- v`, `names(x) <- v`, `attr(x, "k") <- v`:
            // a replacement-function assignment rebinds its first argument.
            Expr::Call { args, .. } => {
                if let Some(first) = args.first() {
                    record_target(&first.value, names);
                }
            }
            _ => {}
        }
    }
    let mut names = HashSet::new();
    // Skips assignment targets (recorded by `record_target` instead);
    // walks nested function bodies, where activators and methods do
    // their rebinding.
    let policy = Walk {
        assign_targets: false,
        assign_operands: false,
        ..Walk::ALL
    };
    let mut visit = |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
        match node {
            AstNode::Stmt(Stmt::Assign { target, .. }) => record_target(target, &mut names),
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::Assign | BinOpKind::SuperAssign,
                lhs,
                ..
            }) => record_target(lhs, &mut names),
            _ => {}
        }
        ControlFlow::Continue(Descend::Into)
    };
    for list in member_lists {
        let _ = walk_expr(list, policy, &mut visit);
    }
    names
}

/// Inferred actuals paired with the call head selected before evaluation.
struct CallResolution {
    arg_types: Vec<RType>,
    head: CallHead,
    locally_shadows_stub: bool,
}

pub(crate) fn is_user_infix_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with('%') && name.ends_with('%')
}

/// The callee of a direct call, spelled as an identifier or a string
/// literal head (R permits `"fn"(...)`). Indirect callees have no name.
pub(crate) fn callee_name(func: &Expr) -> Option<String> {
    match func {
        // R permits a string literal as a call head, e.g. `"[<-"(...)`.
        // Treat it exactly like the corresponding identifier so it takes
        // the normal user-function, typeshed, S3, and higher-order paths.
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name.clone()),
        _ => None,
    }
}

/// The argument bound to the single `x` formal of a dimensional call,
/// under R's exact/partial/positional matching (RY125).
fn dimensional_argument(args: &[Arg]) -> Option<&Arg> {
    let matched = match_arguments(&["x"], args);
    let arg = &args[matched.arg_for_param(0)?];
    (!matches!(arg.value, Expr::Missing(_))).then_some(arg)
}

/// (`package`, `member`) of a `pkg::member` / `pkg:::member` name.
fn qualified_parts(name: &str) -> Option<(&str, &str)> {
    let (pkg, member) = name.rsplit_once("::")?;
    Some((pkg.trim_end_matches(':'), member))
}

/// Expression regions of a cli/glue template: the contents of every
/// top-level `{...}` group that cli evaluates. `{{`/`}}` are escaped
/// literal braces, `{.class content}` markup spans contribute only
/// their nested interpolations (their own content is literal text),
/// and pluralization markers (`{?s}`, `{/...}`) are not expressions.
/// An unbalanced group ends the scan silently.
fn cli_template_regions(text: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    collect_template_regions(text, 0, &mut regions);
    regions
}

fn collect_template_regions(text: &str, base: usize, regions: &mut Vec<(usize, usize)>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' if bytes.get(i + 1) == Some(&b'{') => i += 2,
            b'}' if bytes.get(i + 1) == Some(&b'}') => i += 2,
            b'{' => {
                let Some(end) = matching_brace(text, i) else {
                    return;
                };
                let inner = &text[i + 1..end];
                let trimmed = inner.trim();
                if let Some(content) = markup_span_content(trimmed) {
                    let content_offset = content.as_ptr() as usize - text.as_ptr() as usize;
                    collect_template_regions(content, base + content_offset, regions);
                } else if !trimmed.starts_with(['?', '/']) {
                    regions.push((base + i + 1, base + end));
                }
                i = end + 1;
            }
            _ => i += 1,
        }
    }
}

/// The literal content of a cli markup span `{.class ...}` (one or
/// more leading `.class` tokens followed by whitespace), or `None`
/// when the group is an expression. A lone `{.name}` with no
/// following whitespace is an interpolated variable, not markup.
fn markup_span_content(trimmed: &str) -> Option<&str> {
    let mut rest = trimmed;
    loop {
        let after_dot = rest.strip_prefix('.')?;
        if !after_dot
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            return None;
        }
        let token_end = after_dot
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
            .unwrap_or(after_dot.len());
        let after_token = &after_dot[token_end..];
        if after_token.is_empty() {
            // `{.name}` alone: interpolated variable, not markup.
            return None;
        }
        if !after_token.starts_with(char::is_whitespace) {
            return None;
        }
        rest = after_token.trim_start();
        if !rest.starts_with('.') {
            return Some(rest);
        }
    }
}

/// Byte offset of the `}` matching the `{` at `open`, tracking nested
/// braces and skipping R strings, backtick names, and `#` comments.
/// Markup text is not R code — an apostrophe or `#` in literal prose
/// makes the string-aware scan run off the end — so on failure the
/// scan retries with plain brace counting.
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    matching_brace_scanned(text, open, true).or_else(|| matching_brace_scanned(text, open, false))
}

fn matching_brace_scanned(text: &str, open: usize, code_aware: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' | b'\'' | b'`' if code_aware => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && quote != b'`' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'#' if code_aware => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The two operands an `expect_equal`/`expect_identical` call compares,
/// under R's argument matching (exact, partial, then positional).
fn testthat_compared_operands(args: &[Arg]) -> Option<(&Arg, &Arg)> {
    // R's own matching, so an exact or partial `expected=` claims the
    // formal ahead of a positional argument (`expect_equal(1, 1,
    // expected = 2)` compares 1 with 2, not 1 with 1).
    let matched = match_arguments(&["object", "expected", "..."], args);
    let object = &args[matched.arg_for_param(0)?];
    let expected = &args[matched.arg_for_param(1)?];
    // A duplicated formal is a runtime error in R, not a comparison.
    let duplicated = ["object", "expected"].iter().any(|target| {
        args.iter()
            .filter(|arg| arg.name.as_deref() == Some(target))
            .count()
            > 1
    });
    (!duplicated).then_some((object, expected))
}

/// Structural equivalence for RY122 operands, ignoring spans. Only the
/// variants RY122's safe set admits can compare equal.
fn exprs_equivalent(lhs: &Expr, rhs: &Expr) -> bool {
    match (lhs, rhs) {
        (Expr::Logical(a, _), Expr::Logical(b, _)) => a == b,
        (Expr::Integer(a, _), Expr::Integer(b, _)) => a == b,
        (Expr::Double(a, _), Expr::Double(b, _)) => a.to_bits() == b.to_bits(),
        (Expr::String(a, _), Expr::String(b, _)) => a == b,
        (Expr::Null(_), Expr::Null(_)) => true,
        // Unreachable via RY122 (deterministic_operand excludes `Na`);
        // kept so this helper stays a faithful structural equivalence.
        (Expr::Na(a, _), Expr::Na(b, _)) => a == b,
        (Expr::Ident { name: a, .. }, Expr::Ident { name: b, .. }) => a == b,
        (
            Expr::UnaryOp {
                op: a_op,
                expr: a_expr,
                ..
            },
            Expr::UnaryOp {
                op: b_op,
                expr: b_expr,
                ..
            },
        ) => a_op == b_op && exprs_equivalent(a_expr, b_expr),
        (
            Expr::Index {
                base: a_base,
                kind: a_kind,
                args: a_args,
                ..
            },
            Expr::Index {
                base: b_base,
                kind: b_kind,
                args: b_args,
                ..
            },
        ) => {
            a_kind == b_kind && exprs_equivalent(a_base, b_base) && args_equivalent(a_args, b_args)
        }
        (
            Expr::Call {
                func: a_func,
                args: a_args,
                ..
            },
            Expr::Call {
                func: b_func,
                args: b_args,
                ..
            },
        ) => exprs_equivalent(a_func, b_func) && args_equivalent(a_args, b_args),
        _ => false,
    }
}

fn args_equivalent(lhs: &[Arg], rhs: &[Arg]) -> bool {
    lhs.len() == rhs.len()
        && lhs
            .iter()
            .zip(rhs)
            .all(|(a, b)| a.name == b.name && exprs_equivalent(&a.value, &b.value))
}

/// Whether `expr` reads a variable with a proven class. A base Math
/// call on such a value can dispatch to a method ry has not audited,
/// so RY122 treats the call as unknown. The synthesized `$` field
/// name is not a variable read.
fn reads_known_classed_ident(expr: &Expr, scope: &Scope) -> bool {
    match expr {
        Expr::Ident { name, .. } => scope.get(name).is_some_and(|ty| ty.class.has_known_class()),
        Expr::Index {
            base, kind, args, ..
        } => {
            reads_known_classed_ident(base, scope)
                || proven_classed_element(base, *kind, args, scope)
                || (*kind != IndexKind::Dollar
                    && args
                        .iter()
                        .any(|arg| reads_known_classed_ident(&arg.value, scope)))
        }
        Expr::Call { args, .. } => args
            .iter()
            .any(|arg| reads_known_classed_ident(&arg.value, scope)),
        Expr::UnaryOp { expr, .. } => reads_known_classed_ident(expr, scope),
        _ => false,
    }
}

/// Whether a schema proves the extracted element itself classed (a
/// `Math` method can then run arbitrary code). Unknown element types
/// keep the receiver-only verdict, the same accepted uncertainty as
/// an unknown-class function parameter.
fn proven_classed_element(base: &Expr, kind: IndexKind, args: &[Arg], scope: &Scope) -> bool {
    let Expr::Ident { name, .. } = base else {
        return false;
    };
    let Some(schema) = scope.get(name).and_then(|ty| ty.columns.clone()) else {
        return false;
    };
    let element = match (kind, args) {
        (IndexKind::Dollar, [arg]) => arg.name.as_deref().and_then(|field| {
            schema
                .get(field)
                .or_else(|| schema.unique_prefix_match(field))
        }),
        (IndexKind::Double, [arg]) => match &arg.value {
            Expr::String(field, _) => schema.get(field),
            Expr::Integer(index, _) => schema.get(&format!("[[{index}]]")),
            // `[[1]]` spelled with a double is R's default spelling.
            Expr::Double(value, _) if value.is_finite() && value.fract() == 0.0 => {
                schema.get(&format!("[[{}]]", *value as i64))
            }
            _ => None,
        },
        _ => None,
    };
    element.is_some_and(|ty| ty.class.has_known_class())
}

fn foreach_iteration_bindings(expression: &Expr) -> Option<Vec<String>> {
    let Expr::Call { func, args, .. } = expression else {
        return None;
    };
    match ident_name(func)? {
        "foreach" => Some(
            args.iter()
                .filter_map(|argument| argument.name.as_ref())
                .filter(|name| !name.starts_with('.'))
                .cloned()
                .collect(),
        ),
        "%:%" if args.len() == 2 => {
            let mut bindings = foreach_iteration_bindings(&args[0].value)?;
            bindings.extend(foreach_iteration_bindings(&args[1].value)?);
            Some(bindings)
        }
        _ => None,
    }
}

fn injected_string_bindings(expression: &Expr) -> Vec<String> {
    match expression {
        Expr::String(name, _) => vec![name.clone()],
        Expr::Call { func, args, .. } if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "c") => {
            args.iter()
                .flat_map(|argument| injected_string_bindings(&argument.value))
                .collect()
        }
        _ => Vec::new(),
    }
}

fn printf_argument_count(format: &str) -> Option<usize> {
    let bytes = format.as_bytes();
    let mut index = 0;
    let mut count = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        index += 1;
        if bytes.get(index) == Some(&b'%') {
            index += 1;
            continue;
        }
        while let Some(byte) = bytes.get(index).copied() {
            if byte == b'*' || byte == b'$' {
                return None;
            }
            index += 1;
            if byte.is_ascii_alphabetic() {
                count += 1;
                break;
            }
        }
    }
    Some(count)
}

fn scope_effect_populates_current_scope(
    signature: &FunctionSig,
    args: &[Arg],
    top_level: bool,
) -> bool {
    if matches!(signature.scope_effect, Some(ScopeEffect::UnknownBindings)) {
        return true;
    }
    let Some(ConditionalScopeEffect {
        effect: ScopeEffect::UnknownBindings,
        current_scope_when,
        default_current_scope: DefaultCurrentScope::TopLevel,
    }) = signature.conditional_scope_effect.as_ref()
    else {
        return false;
    };
    let Some(index) = bound_argument_index(&signature.params, args, &current_scope_when.param)
    else {
        return top_level;
    };
    match &args[index].value {
        Expr::Logical(value, _) => *value == current_scope_when.equals,
        // An environment-valued or opaque control can be the caller frame;
        // preserve the loader's conservative, silence-over-noise policy.
        _ => true,
    }
}

fn assertion_is_provenanced(signature: &FunctionSig, assertion: &AssertionSpec) -> bool {
    matches!(
        assertion.provenance.kind,
        AssertionProvenanceKind::StandaloneTypesCheck
    ) && assertion.provenance.fingerprint_params == ["arg", "call"]
        && assertion
            .provenance
            .fingerprint_params
            .iter()
            .all(|fingerprint| {
                signature
                    .params
                    .iter()
                    .any(|param| param.name == *fingerprint)
            })
}
