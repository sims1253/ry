use super::*;
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
        // A call to a function literal (IIFE) never reaches the
        // name-based stages below.
        if let Expr::Function { .. } = func {
            return self.infer_function_literal_call(func, args, scope);
        }

        // Only model direct calls `name(...)`. Pipelines and indirect calls
        // return opaque.
        let Some(name) = callee_name(func) else {
            return self.infer_opaque_callee(func, args, scope, span);
        };

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

        // For namespace-qualified calls (`pkg::fn(args)`), strip the
        // package prefix for the lookups below, so `stats::rnorm(10)`
        // resolves the same way `rnorm(10)` does. The special-case
        // string-equality checks keep using the full `name`, because
        // those builtins are always invoked unqualified. `bare_name`
        // handles both `::` and `:::` forms.
        let semantic_name = scope.function_alias(&name).unwrap_or(&name).to_string();
        let lookup_name = if is_user_infix_name(&semantic_name) {
            // `%::%` is an infix operator, not a namespace-qualified call.
            // Do not split the `::` embedded in its operator spelling.
            semantic_name.clone()
        } else {
            crate::semantic_lists::bare_name(&semantic_name).to_string()
        };

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

        self.check_comparison_inside_aggregate(&lookup_name, args);
        self.check_comparison_inside_math_fn(&lookup_name, args);
        self.check_identical_list_subset(&lookup_name, args, scope);

        if let Some(t) = self.infer_deferred_call(&lookup_name, args, scope, span) {
            return t;
        }

        self.check_printf_format_arity(&lookup_name, args);

        if let Some(t) =
            self.infer_nse_quoting_call(&semantic_name, &lookup_name, args, scope, span)
        {
            return t;
        }

        // `switch(EXPR, ...)`: the join of all alternatives.
        if semantic_name == "switch" {
            return self.infer_switch_call(args, scope);
        }

        // `tryCatch(expr, ..., handler = fun)`: the join of the main
        // expression and all handler return types.
        if semantic_name == "tryCatch" {
            return self.infer_trycatch_call(args, scope);
        }

        // The class-constructor stage: `structure`, `factor`, S4 `new`.
        if let Some(t) =
            self.infer_class_constructor_call(&semantic_name, &lookup_name, args, scope)
        {
            return t;
        }

        // The default two-argument `assign` rebinds in the current
        // environment.
        if let Some(t) = self.infer_local_assign_call(&semantic_name, args, scope) {
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
        let mut call = self.infer_argument_types(&name, &semantic_name, &lookup_name, args, scope);

        // The argument-validation stage.
        self.check_call_arguments(&lookup_name, &call, args, span);

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

        // The lexical-callable stage. Must run before the constructor and
        // dispatch stages below so a local binding shadows a same-named
        // builtin (R's lexical scoping).
        if let Some(t) = self.infer_lexical_callable_call(
            &name,
            &lookup_name,
            args,
            &call.arg_types,
            span,
            scope,
        ) {
            return t;
        }

        // No lexical callable won. A bare typed package value is therefore a
        // non-function call, not a zero-argument function signature.
        if scope.get(&lookup_name).is_none()
            && let Some(value_type) = self.resolve_typeshed_value(&name)
        {
            return self.emit_not_callable(&name, value_type.mode, span);
        }

        // The atomic-constructor stage: `c`, `list`, `data.frame`, `t`,
        // `as.data.frame`.
        if let Some(t) = self.infer_atomic_constructor_call(&lookup_name, args, &call.arg_types) {
            return t;
        }

        // Class dispatch must run before the higher-order, FnTable, and
        // typeshed stages below: a method's inferred return type wins over
        // the generic's stub.
        if let Some(rt) = self.try_s4_dispatch(&lookup_name, &call.arg_types) {
            return rt;
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
        if call
            .resolved_sig
            .as_ref()
            .is_some_and(|signature| signature.higher_order.is_some())
        {
            self.walk_callback_for_diagnostics(&lookup_name, args, &call.arg_types, scope);
        }
        if let Some(rt) =
            self.infer_higher_order_call(&lookup_name, args, &call.arg_types, scope, span)
        {
            return rt;
        }

        // User functions: the refined FnTable return slot (stabilized by
        // the fixpoint loop in `check()`; refining on demand would risk
        // exponential blowup).
        if let Some(function) = call.user_function.as_ref() {
            return self.return_slots.get(function.return_slot);
        }

        // The literal-length constructor stage: `vector`, `rep`, `seq`.
        // Must run after the FnTable stage above so a user-defined
        // `rep`/`seq` still wins, and before the typeshed stage below so
        // the literal-pinned length beats the conservative stub.
        if let Some(t) = self.infer_literal_length_call(&lookup_name, args, &call.arg_types) {
            return t;
        }

        // The typeshed stage: a qualified call (`pkg::fun`) resolves
        // against `load_package(pkg)`; an unqualified call falls back
        // from base to loaded packages (reverse load order).
        if let Some(sig) = call.resolved_sig {
            return self.apply_sig(&sig, &call.arg_types, args);
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
        self.infer_args_for_diagnostics(args, scope);
        RType::unknown()
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
        let mut local = scope.clone();
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
        let custom_infix_is_known = self.has_function_anywhere(semantic_name)
            || self
                .fn_table
                .fns
                .keys()
                .any(|name| semantic_argument_name(name) == semantic_name);
        if (is_user_infix_name(semantic_name) || semantic_name == ".") && !custom_infix_is_known {
            let mut quoted_scope = scope.clone();
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
            let mut child = scope.clone();
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
    fn check_comparison_inside_aggregate(&mut self, lookup_name: &str, args: &[Arg]) {
        if matches!(lookup_name, "length" | "nchar")
            && let Some(Expr::BinOp {
                op,
                span: comparison_span,
                ..
            }) = args.first().map(|arg| &arg.value)
            && is_comparison(*op)
        {
            let message = format!(
                "comparison is inside `{lookup_name}()`; compare `{lookup_name}(x)` instead"
            );
            self.emit(Severity::Warning, *comparison_span, "RY093", message);
        }
    }

    /// RY100: numeric math functions coerce logical comparisons to 0/1,
    /// which is almost always a misplaced parenthesis (`abs(x > y)` rather
    /// than `abs(x) > y`). Extra parentheses do not change the parsed
    /// argument, so deliberately parenthesized comparisons remain visible
    /// here.
    fn check_comparison_inside_math_fn(&mut self, lookup_name: &str, args: &[Arg]) {
        if matches!(
            lookup_name,
            "abs"
                | "sqrt"
                | "exp"
                | "log"
                | "log2"
                | "log10"
                | "log1p"
                | "floor"
                | "ceiling"
                | "round"
                | "trunc"
        ) && let Some(Expr::BinOp {
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

    /// `hasArg` (captures its argument name) and `on.exit` (evaluates
    /// `expr` when the enclosing function returns).
    fn infer_deferred_call(
        &mut self,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
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
            }) && let Some(formals) = self.enclosing_formals.last()
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
                    let mut exit_scope = scope.clone();
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
            && args.len().saturating_sub(1) < required
        {
            self.emit(
                Severity::Warning,
                *format_span,
                "RY094",
                format!(
                    "format string requires {required} value argument(s), but {} provided",
                    args.len().saturating_sub(1)
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
        // `library(foo)` and `require(foo)` take a package name as a bare
        // symbol, not an expression. Inferring their args would trigger
        // spurious RY010 on every `library(magrittr)` etc. We ALSO record
        // the package name into `self.loaded` so the dplyr NSE gating can
        // treat dplyr/tidyverse as in scope after either call.
        if semantic_name == "library" || semantic_name == "require" {
            if let Some(first) = args.first() {
                let character_only = args.iter().any(|argument| {
                    argument.name.as_deref() == Some("character.only")
                        && matches!(argument.value, Expr::Logical(true, _))
                });
                let package = match &first.value {
                    Expr::Ident { name, .. } if !character_only => Some(name),
                    Expr::String(name, _) => Some(name),
                    _ => None,
                };
                if let Some(pkg) = package {
                    Arc::make_mut(&mut self.loaded).insert(pkg.clone());
                    Arc::make_mut(&mut self.bare_loaded).insert(pkg.clone());
                    // An attached package without a stub can contribute any
                    // export or lazy-loaded dataset to the search path.
                    if !self.package_is_known(pkg) {
                        scope.mark_search_path_unknown();
                    }
                } else if character_only {
                    // `library(pkg, character.only = TRUE)` evaluates its
                    // argument. Without a literal package name we cannot
                    // know which bindings were attached.
                    scope.mark_search_path_unknown();
                }
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
        // statements. Package/control arguments are not introduced.
        if semantic_name == "data" {
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

        if semantic_name == "load" {
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
            || (is_registered_ffi_wrapper(semantic_name) && self.native_registration)
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
    ) -> CallResolution {
        let inherited_sig = self.resolve_user_s3_inherited_sig(lookup_name);
        let inherited_s3_metadata = inherited_sig.is_some();
        let resolved_sig = self.resolve_typeshed_sig(semantic_name).or(inherited_sig);
        // Formula interfaces can name a later `data` argument as the source
        // of their data mask. Infer it once up front so earlier `weights`,
        // `subset`, and similar arguments see the right scope.
        let supplied_data_mask_source = resolved_sig.as_ref().and_then(|signature| {
            data_mask_source_arg(signature, args).map(|argument_index| {
                (
                    argument_index,
                    self.infer(&args[argument_index].value, scope),
                )
            })
        });
        // Function definitions may use a quoted binding name (`'%as%' <-
        // function(...)`), whereas an infix call is looked up as `%as%`.
        // Match the normalized spelling as well so user-function metadata
        // (notably NSE/quoting parameters) reaches those calls.
        // A function value in the current lexical scope shadows the flat
        // project function table. The table intentionally indexes by name and
        // may contain a same-named nested/top-level definition from elsewhere;
        // using that signature here produces bogus RY090/RY091 diagnostics.
        let lexical_callable = !name.contains("::") && scope.is_lexical_function(lookup_name);
        let user_function = if lexical_callable {
            None
        } else {
            self.fn_table.fns.get(lookup_name).cloned().or_else(|| {
                self.fn_table
                    .fns
                    .iter()
                    .find(|(name, _)| semantic_argument_name(name) == lookup_name)
                    .map(|(_, function)| function.clone())
            })
        };
        let user_argument_matches = user_function.as_ref().map(|function| {
            let names: Vec<&str> = function
                .params
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect();
            match_arguments(&names, args)
        });
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
            let user_dispatch = inherited_s3_metadata
                || user_function.is_some()
                || arg_types
                    .first()
                    .is_some_and(|first| self.resolves_user_s3_dispatch(lookup_name, first));
            // The user formal this actual bound to (directly or through
            // `...`), looked up once for both the defusing and quoting
            // flags below.
            let user_param = user_argument_matches
                .as_ref()
                .and_then(|matches| matches.param_for_arg[index].or(matches.dots))
                .and_then(|parameter| user_function.as_ref()?.params.get(parameter));
            let is_defused = user_param.is_some_and(|parameter| parameter.defused);
            let is_quoting = user_param.is_some_and(|parameter| parameter.quoting);
            if is_quoting {
                // User functions that capture an argument with substitute(),
                // bquote(), or match.call()-style reflection receive the
                // expression unevaluated. Infer it without diagnostics so
                // nested operations and names cannot be mistaken for runtime
                // R code.
                let mut quoted_scope = scope.clone();
                self.infer_discarding(&a.value, &mut quoted_scope);
                arg_types.push(RType::unknown());
                continue;
            }
            if is_defused && declared_mode.is_none_or(|mode| matches!(mode, EvalMode::Normal)) {
                let mut local = self.dplyr_data_mask_scope(scope, &RType::unknown());
                arg_types.push(self.infer(&a.value, &mut local));
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
                    EvalMode::Normal => self.infer(&a.value, scope),
                    EvalMode::QuotedSymbol => {
                        if matches!(a.value, Expr::Ident { .. }) {
                            RType::unknown()
                        } else {
                            self.infer_discarding(&a.value, scope)
                        }
                    }
                    EvalMode::QuotedExpression | EvalMode::CapturesPromise => RType::unknown(),
                    EvalMode::DataMask => {
                        // A declared source is conditional: without a
                        // supplied `data` argument, formula extras evaluate
                        // normally in the caller environment.
                        let Some(data) = supplied_data_mask_source
                            .as_ref()
                            .map(|(_, data)| data.clone())
                            .or_else(|| {
                                resolved_sig
                                    .as_ref()
                                    .is_some_and(|signature| signature.data_mask_source.is_none())
                                    .then(|| {
                                        arg_types.first().cloned().unwrap_or_else(RType::unknown)
                                    })
                            })
                        else {
                            arg_types.push(self.infer(&a.value, scope));
                            continue;
                        };
                        let mut local = self.dplyr_data_mask_scope(scope, &data);
                        local.insert(".", RType::unknown());
                        if user_dispatch {
                            local = local.with_unknown_data_mask();
                        }
                        self.infer(&a.value, &mut local)
                    }
                    EvalMode::TidySelect => {
                        let data = arg_types.first().cloned().unwrap_or_else(RType::unknown);
                        let mut local = self.dplyr_data_mask_scope(scope, &data);
                        if user_dispatch {
                            local = local.with_unknown_data_mask();
                        }
                        self.infer_tidyselect_expr(&a.value, &mut local)
                    }
                };
                arg_types.push(inferred);
            } else {
                arg_types.push(self.infer(&a.value, scope));
            }
        }
        CallResolution {
            arg_types,
            resolved_sig,
            user_function,
            lexical_callable,
            locally_shadows_stub: false,
        }
    }

    /// The argument-validation stage: matching is validated only for
    /// signatures whose origin is known. A user definition shadows a
    /// same-named stub. A lexical function binding (a function literal
    /// defined inside an enclosing function body) shadows both the flat
    /// project table and the typeshed/base signature, so neither is
    /// consulted for it — checking `inherits("x")` against
    /// `base::inherits` is a lookup-order bug, not a missing argument.
    fn check_call_arguments(
        &mut self,
        lookup_name: &str,
        resolution: &CallResolution,
        args: &[Arg],
        span: Span,
    ) {
        if self.validate_user_call_arguments {
            if let Some(user_function) = resolution.user_function.as_ref() {
                self.check_user_call_arguments(lookup_name, user_function, args, span);
            } else if !resolution.lexical_callable
                && let Some(signature) = resolution.resolved_sig.as_ref()
            {
                self.check_typeshed_call_arguments(
                    lookup_name,
                    signature,
                    args,
                    &resolution.arg_types,
                    span,
                );
            }
        } else if !resolution.lexical_callable
            && !self.fn_table.fns.contains_key(lookup_name)
            && let Some(signature) = resolution.resolved_sig.as_ref()
        {
            self.check_typeshed_call_arguments(
                lookup_name,
                signature,
                args,
                &resolution.arg_types,
                span,
            );
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
            && (name.contains("::") || resolution.user_function.is_none())
            && resolution.resolved_sig.as_ref().is_some_and(|signature| {
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
        let assertion_signature = resolution.resolved_sig.as_ref().or_else(|| {
            (!name.contains("::") && (!locally_shadows_stub || resolution.user_function.is_some()))
                .then(|| ry_typeshed::load_package("rlang"))
                .flatten()
                .and_then(|typeshed| typeshed.functions.get(lookup_name))
        });
        // The assertion path consults the argument match three times. Build it
        // once, and only for signatures that actually declare an assertion.
        let assertion_binding = assertion_signature
            .filter(|signature| signature.assertion.is_some())
            .map(|signature| (signature, match_params(&signature.params, args)));
        if (name.contains("::") || !locally_shadows_stub || resolution.user_function.is_some())
            && let Some((signature, bindings)) = assertion_binding
            && let Some(assertion) = signature.assertion.as_ref()
            && assertion_is_provenanced(signature, assertion)
            && resolution.user_function.as_ref().is_none_or(|function| {
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
            let mut target = json_rtype_to_rtype(&assertion.target);
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
            for argument in args {
                if name.ends_with("assert_that") && argument.name.as_deref() == Some("msg") {
                    continue;
                }
                let narrowing = self.extract_type_narrowing(&argument.value, scope);
                let (positive_scope, _, _) = apply_narrowing(scope, &narrowing);
                *scope = positive_scope;
            }
        }
    }

    /// The lexical-callable stage: a `Function`-typed scope binding with an
    /// inferred `fn_sig` resolves to the signature's return type (this is
    /// what makes `c <- make_counter(); v <- c()` work). Qualified calls
    /// bypass local bindings: `pkg::f()` selects `f` from `pkg`.
    fn infer_lexical_callable_call(
        &mut self,
        name: &str,
        lookup_name: &str,
        args: &[Arg],
        arg_types: &[RType],
        span: Span,
        scope: &mut Scope,
    ) -> Option<RType> {
        if name.contains("::") {
            return None;
        }
        let t = scope.get(lookup_name)?;
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
        if !matches!(t.mode, Mode::Opaque) {
            // R's function/value namespace separation: when a name is
            // CALLED, R searches the environment chain for a *function*
            // named `name` and skips non-function bindings. So a local
            // non-function binding (e.g. `lengths <- lengths(x)`) does
            // NOT shadow a same-named function in the typeshed or
            // FnTable at a call site. If such a function exists, fall
            // through to the resolution below instead of firing RY070.
            // Only when no function of that name exists anywhere does
            // calling the non-function value warrant RY070.
            // A concrete lexical value at this point wins over the
            // whole-project callable inventory; a later or cross-file
            // S7 constructor must not hide this proven call error.
            let has_function_elsewhere = self.has_function_anywhere(name)
                && (scope.is_default_parameter(name)
                    || !self.fn_table.callable_vars.contains(name));
            if !has_function_elsewhere {
                // RY070: a non-function value is being called as if it
                // were a function. R errors at runtime with
                // "could not find function". Args have already been
                // inferred above, so we just emit and return opaque
                // (re-inferring would double-emit arg diagnostics).
                return Some(self.emit_not_callable(name, t.mode, span));
            }
            // A function exists elsewhere; fall through to resolve it
            // (the local non-function binding is ignored at the call
            // site, matching R).
        }
        // Opaque: fall through; the name might still resolve via
        // the FnTable or typeshed below.
        None
    }

    /// Returns a value for a union call only when every member is a closure.
    /// A NULL/function union deliberately stays non-callable: the NULL arm is
    /// an unguarded runtime error, not an overload.
    fn callable_function_union(
        &mut self,
        ty: &RType,
        args: &[Arg],
        arg_types: &[RType],
    ) -> Option<RType> {
        let members = ty.members.as_ref()?;
        if ty.mode != Mode::Union
            || members.is_empty()
            || members.iter().any(|member| member.mode != Mode::Function)
        {
            return None;
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

    /// Emit RY070 for a call to a name whose type is known to be a
    /// non-function value, and return the opaque result every such site
    /// yields.
    fn emit_not_callable(&mut self, name: &str, mode: Mode, span: Span) -> RType {
        self.emit(
            Severity::Error,
            span,
            "RY070",
            format!("`{}` is `{}`, not a function; cannot call it", name, mode),
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

    pub(crate) fn try_s4_dispatch(&self, generic: &str, arg_types: &[RType]) -> Option<RType> {
        let class = arg_types.first()?.class.first()?;
        let slot = self
            .fn_table
            .s4_methods
            .get(&(generic.to_string(), class.to_string()))?;
        Some(self.return_slots.get(*slot))
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
                let mut probe = scope.clone();
                self.infer_discarding(other, &mut probe)
            }
        }
    }

    fn infer_injected_expr(&mut self, expr: &Expr, scope: &mut Scope) -> RType {
        match expr {
            Expr::Function { params, body, .. } => {
                let mut inner = scope.clone();
                for parameter in params {
                    inner.insert_parameter(parameter.name.clone(), RType::unknown());
                }
                for name in assigned_names_in_body(body) {
                    inner.insert(name, RType::unknown());
                }
                for statement in body {
                    self.walk_stmt(statement, &mut inner, None);
                }
                RType::scalar(Mode::Function)
            }
            Expr::Call { args, .. } => {
                for argument in args {
                    self.infer_injected_expr(&argument.value, scope);
                }
                RType::unknown()
            }
            Expr::Block { body, .. } => {
                for statement in body {
                    self.walk_stmt(statement, scope, None);
                }
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

/// The per-call resolution state produced by the argument-inference stage
/// and shared by every stage after it.
struct CallResolution {
    /// The inferred argument types, honoring the declared eval modes.
    arg_types: Vec<RType>,
    /// The typeshed (or inherited-S3) signature for the call, if any.
    resolved_sig: Option<FunctionSig>,
    /// The project FnTable entry for the call, unless a lexical callable
    /// shadows it.
    user_function: Option<UserFn>,
    /// Whether a lexical function binding shadows every table lookup.
    lexical_callable: bool,
    /// Whether a local non-alias binding shadows a same-named stub; the
    /// stub-assertion stage consults it.
    locally_shadows_stub: bool,
}

fn is_user_infix_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with('%') && name.ends_with('%')
}

/// The callee of a direct call, spelled as an identifier or a string
/// literal head (R permits `"fn"(...)`). Indirect callees have no name.
fn callee_name(func: &Expr) -> Option<String> {
    match func {
        // R permits a string literal as a call head, e.g. `"[<-"(...)`.
        // Treat it exactly like the corresponding identifier so it takes
        // the normal user-function, typeshed, S3, and higher-order paths.
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name.clone()),
        _ => None,
    }
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
