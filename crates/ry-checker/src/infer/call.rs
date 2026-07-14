use super::*;

impl Checker {
    pub(crate) fn infer_call(
        &mut self,
        func: &Expr,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        // Handle calls to function literals (IIFEs):
        // `(function(x) x + 1)(2L)`. Infer the return type using the
        // actual argument types via callback_return_type, which walks
        // the body with the params bound to the argument types.
        if let Expr::Function { .. } = func {
            let arg_types: Vec<RType> = args.iter().map(|a| self.infer(&a.value, scope)).collect();
            if let Some(rt) = self.callback_return_type(func, &arg_types, scope) {
                return rt;
            }
            return RType::unknown();
        }

        // Only model direct calls `name(...)`. Pipelines and indirect calls
        // return opaque.
        let name = match func {
            Expr::Ident { name, .. } => name.clone(),
            _ => {
                // Calling a literal value (`42()`, `"x"()`, `TRUE()`,
                // `NULL()`) is always a runtime error in R ("attempt to
                // apply non-function"). Flag it. Other
                // non-Ident callees (index expressions, calls returning
                // functions) stay silent as before.
                if let Some(mode) = literal_callee_mode(func) {
                    self.emit(
                        Severity::Error,
                        span,
                        "RY070",
                        format!("cannot call a value of mode `{}`", mode),
                    );
                    for a in args {
                        self.infer(&a.value, scope);
                    }
                    return RType::unknown();
                }
                self.infer(func, scope);
                for a in args {
                    self.infer(&a.value, scope);
                }
                return RType::unknown();
            }
        };

        // For namespace-qualified calls (`pkg::fn(args)`), strip the
        // package prefix for the typeshed / FnTable / higher-order /
        // S3-generic lookups below, so `stats::rnorm(10)` resolves the
        // same way `rnorm(10)` does. The special-case string-equality
        // checks (library, switch, structure, factor, the dplyr NSE
        // verbs, ...) keep using the full `name`, because those
        // builtins are always invoked unqualified; a qualified call
        // like `base::c(...)` falls through to the typeshed lookup
        // with the stripped name. `rsplit_once("::")` handles both
        // `::` and `:::` forms: for `pkg:::fn` it splits at the last
        // `::`, yielding `("pkg:", "fn")`.
        let semantic_name = scope.function_alias(&name).unwrap_or(&name).to_string();
        let lookup_name = semantic_name
            .rsplit_once("::")
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| semantic_name.clone());

        // `foreach(iter = xs, ...) %op% { ... }` evaluates the RHS with
        // each named iteration argument bound. Operator aliases are common,
        // so recognize the foreach-shaped LHS rather than a fixed `%do%` or
        // `%dopar%` spelling. `%:%` chains contribute bindings from every
        // constituent foreach call.
        if is_user_infix_name(&semantic_name)
            && args.len() == 2
            && let Some(bindings) = foreach_iteration_bindings(&args[0].value)
        {
            let _ = self.infer(&args[0].value, scope);
            let mut local = scope.clone();
            for binding in bindings {
                local.insert(binding, RType::unknown());
            }
            return self.infer(&args[1].value, &mut local);
        }

        if let Some(result) =
            self.infer_injected_call(&semantic_name, &lookup_name, args, scope, span)
        {
            return result;
        }

        if lookup_name == "assign"
            && args.iter().any(|arg| {
                arg.name.as_deref() == Some("envir")
                    && matches!(
                        &arg.value,
                        Expr::Call { func, .. }
                            if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "asNamespace")
                    )
            })
            && let Some(binding) = args.first().and_then(|arg| match &arg.value {
                Expr::String(name, _) => Some(name.clone()),
                _ => None,
            })
        {
            for argument in args.iter().skip(1) {
                self.infer(&argument.value, scope);
            }
            scope.insert(binding, RType::unknown());
            return RType::unknown();
        }

        // `sum(x > 0)` is the idiomatic R way to count matches, so `sum`
        // is deliberately excluded from this mis-parenthesization family.
        if matches!(lookup_name.as_str(), "length" | "nchar" | "abs")
            && let Some(Expr::BinOp { op, span, .. }) = args.first().map(|arg| &arg.value)
            && matches!(
                op,
                BinOpKind::Lt
                    | BinOpKind::Le
                    | BinOpKind::Gt
                    | BinOpKind::Ge
                    | BinOpKind::Eq
                    | BinOpKind::Ne
            )
        {
            self.emit(
                Severity::Warning,
                *span,
                "RY093",
                format!(
                    "comparison is inside `{lookup_name}()`; compare `{lookup_name}(x)` instead"
                ),
            );
        }

        // `hasArg` captures its argument name rather than evaluating it.
        // Model that quoting here so a non-formal does not also produce RY010.
        // With `...` in the formals, `hasArg(name)` legitimately matches
        // dots-supplied arguments (`if (hasArg(b)) list(...)$b` idiom), so
        // only a function without `...` makes the check provably FALSE.
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
            return RType::scalar(Mode::Logical);
        }

        if matches!(lookup_name.as_str(), "sprintf" | "gettextf")
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

        // NSE-opaque functions whose arguments are not regular values:
        // `library(foo)` and `require(foo)` take a package name as a bare
        // symbol, not an expression. Inferring their args would trigger
        // spurious RY010 on every `library(magrittr)` etc. We ALSO record
        // the package name into `self.loaded` so the dplyr NSE gating can
        // treat dplyr/tidyverse as in scope after either call.
        if semantic_name == "library" || semantic_name == "require" {
            if let Some(first) = args.first() {
                if let Expr::Ident { name: pkg, .. } = &first.value {
                    Arc::make_mut(&mut self.loaded).insert(pkg.clone());
                }
            }
            return if semantic_name == "require" {
                RType::new(Mode::Logical, Length::One)
            } else {
                RType::new(Mode::Null, Length::Zero)
            };
        }

        // Formula construction and expression-vector constructors quote
        // their language arguments. Names inside them are resolved later in
        // a model/data environment, not at construction time.
        if matches!(lookup_name.as_str(), "~" | "expression" | "vars") {
            return RType::unknown();
        }

        // `data(name)` loads one or more datasets into the current
        // environment. Bare names and string literals are data identifiers,
        // not reads of existing variables, and become bindings for following
        // statements. Package/control arguments are not introduced.
        if semantic_name == "data" {
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
            return RType::new(Mode::Character, Length::Unknown);
        }

        if semantic_name == "load" {
            for argument in args {
                let _ = self.infer(&argument.value, scope);
            }
            if let Some(bindings) = self.load_bindings.get(&span.start).cloned() {
                for binding in bindings {
                    scope.insert(binding, RType::unknown());
                }
            }
            return RType::new(Mode::Character, Length::Unknown);
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
        if is_ffi_primitive(&semantic_name) {
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
            return RType::unknown();
        }

        // NSE-symbol functions: take bare symbol arguments that should
        // NOT be resolved as variable references. These are commonly
        // used in metaprogramming and NSE contexts where the argument
        // is a name, not a value. We return opaque without evaluating
        // the args as expressions, suppressing spurious RY010.
        if is_nse_symbol_fn(&lookup_name) {
            return RType::unknown();
        }

        // `switch(EXPR, ...)` selects one of several alternatives.
        // The result type is the join of all alternatives. Both numeric
        // switch (`switch(1, "a", "b")`) and named switch
        // (`switch(x, a = 1, b = 2)`) are supported.
        if semantic_name == "switch" {
            return self.infer_switch_call(args, scope, span);
        }

        // `tryCatch(expr, ..., handler = fun)`: error-handling construct.
        // The result type is the join of the main expression and all
        // handler return types. Handlers are named arguments whose
        // values are functions (error = function(e) ...).
        if semantic_name == "tryCatch" {
            return self.infer_trycatch_call(args, scope, span);
        }

        // `structure(x, class = "...")` is R's class constructor. We
        // model only the common literal forms:
        //   * `class = "foo"` attaches a single class.
        //   * `class = c("a", "b", ...)` attaches a class vector.
        // Non-literal or unparseable forms fall through to opaque
        // inference with `ClassVector::unknown()` so RY050 stays quiet.
        if semantic_name == "structure" {
            return self.infer_structure_call(args, scope, span);
        }
        // `factor(x)` returns an integer vector with class "factor".
        // (And often also "ordered" if `ordered = TRUE`, but we keep v1
        // to the base case.)
        if semantic_name == "factor" {
            // Infer args so unbound-variable diagnostics still fire.
            for a in args {
                let _ = self.infer(&a.value, scope);
            }
            return RType::new(Mode::Integer, Length::Unknown)
                .with_class(ClassVector::single("factor"));
        }
        if lookup_name == "new" {
            for argument in args.iter().skip(1) {
                let _ = self.infer(&argument.value, scope);
            }
            return args
                .first()
                .and_then(|argument| match &argument.value {
                    Expr::String(class, _) => {
                        Some(RType::unknown().with_class(ClassVector::single(class)))
                    }
                    _ => None,
                })
                .unwrap_or_else(RType::unknown);
        }

        // The default two-argument form assigns into the current
        // environment. A literal name makes that binding fully static.
        if semantic_name == "assign" && args.len() == 2 {
            let name = match &args[0].value {
                Expr::String(name, _) => Some(name.clone()),
                _ => None,
            };
            let _ = self.infer(&args[0].value, scope);
            let value = self.infer(&args[1].value, scope);
            if let Some(name) = name {
                scope.insert(name, value.clone());
            }
            return value;
        }

        // NSE verbs (`subset`, `with`, `within`, `transform`) evaluate
        // their expression arguments in an augmented scope where the
        // data frame's columns are bound as names. We must intercept
        // these BEFORE the eager `infer(&a.value, scope)` loop below,
        // because that loop would emit spurious RY010 ("variable not
        // bound") for every column reference (`cyl`, `mpg`, ...).
        // Returns `Some(t)` when the call was handled; the caller uses
        // the returned type verbatim. Returns `None` to fall through to
        // the regular arg-inference path.
        if let Some(t) = self.infer_schema_call(&semantic_name, args, scope, span) {
            return t;
        }

        // Infer argument types, honoring declarative per-parameter
        // evaluation modes from the typeshed. Package APIs can opt into
        // quoted symbols, data masks, or tidy-select without adding their
        // names to the checker engine.
        let inherited_sig = self.resolve_user_s3_inherited_sig(&lookup_name);
        let inherited_s3_metadata = inherited_sig.is_some();
        let resolved_sig = self.resolve_typeshed_sig(&semantic_name).or(inherited_sig);
        let user_function = self.fn_table.fns.get(&lookup_name).cloned();
        let user_argument_matches = user_function.as_ref().map(|function| {
            let names: Vec<&str> = function
                .params
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect();
            match_arguments(&names, args)
        });
        let mut arg_types: Vec<RType> = Vec::with_capacity(args.len());
        for (index, a) in args.iter().enumerate() {
            let declared_mode = resolved_sig
                .as_ref()
                .and_then(|signature| argument_eval_mode(signature, args, index));
            let user_dispatch = inherited_s3_metadata
                || user_function.is_some()
                || arg_types
                    .first()
                    .is_some_and(|first| self.resolves_user_s3_dispatch(&lookup_name, first));
            let is_defused = user_argument_matches
                .as_ref()
                .and_then(|matches| matches.param_for_arg[index].or(matches.dots))
                .and_then(|parameter| user_function.as_ref()?.params.get(parameter))
                .is_some_and(|parameter| parameter.defused);
            if is_defused && declared_mode.is_none_or(|mode| matches!(mode, EvalMode::Normal)) {
                let mut local = self.dplyr_data_mask_scope(scope, &RType::unknown());
                arg_types.push(self.infer(&a.value, &mut local));
                continue;
            }
            if let Some(mode) = declared_mode {
                let inferred = match mode {
                    EvalMode::Normal => self.infer(&a.value, scope),
                    EvalMode::QuotedSymbol => {
                        if matches!(a.value, Expr::Ident { .. }) {
                            RType::unknown()
                        } else {
                            self.infer(&a.value, scope)
                        }
                    }
                    EvalMode::QuotedExpression => RType::unknown(),
                    EvalMode::DataMask => {
                        let data = arg_types.first().cloned().unwrap_or_else(RType::unknown);
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
        // Validate ordinary R argument matching only for signatures whose
        // origin is known. A user definition shadows a same-named stub.
        if self.validate_user_call_arguments {
            if let Some(user_function) = user_function.as_ref() {
                self.check_user_call_arguments(&lookup_name, user_function, args, span);
            } else if let Some(signature) = resolved_sig.as_ref() {
                self.check_typeshed_call_arguments(&lookup_name, signature, args, &arg_types, span);
            }
        } else if !self.fn_table.fns.contains_key(&lookup_name)
            && let Some(signature) = resolved_sig.as_ref()
        {
            self.check_typeshed_call_arguments(&lookup_name, signature, args, &arg_types, span);
        }
        let locally_shadows_stub = !name.contains("::")
            && scope.get(&name).is_some()
            && scope.function_alias(&name).is_none();
        if !locally_shadows_stub
            && (name.contains("::") || user_function.is_none())
            && resolved_sig.as_ref().is_some_and(|signature| {
                matches!(signature.scope_effect, Some(ScopeEffect::UnknownBindings))
            })
        {
            // The resolved function can add names that static analysis cannot
            // enumerate (for example base::attach() or Rcpp::sourceCpp()).
            // The marker is inherited by scopes cloned after this call.
            scope.mark_search_path_unknown();
        }
        if let Some(target) = assertion_call_target(&lookup_name) {
            if let Some(Expr::Ident { name: var, .. }) = args.first().map(|a| &a.value) {
                scope.insert(var.clone(), target);
            }
            return RType::new(Mode::Null, Length::Zero);
        }

        // Indirect call through a closure value: if the name is bound
        // in scope to a `Function`-typed value with an inferred
        // `fn_sig`, the call resolves to the signature's return type.
        // This is what makes `c <- make_counter(); v <- c()` work
        // without `c` having its own FnTable entry. We check this
        // before the FnTable / typeshed paths so a local binding
        // shadows any same-named top-level function (matching R's
        // lexical scoping).
        //
        // Namespace-qualified calls bypass local lexical bindings:
        // `pkg::f()` selects `f` from `pkg`, so a local argument named
        // `f` must not make the qualified call look like a non-function.
        if !name.contains("::") {
            if let Some(t) = scope.get(&lookup_name) {
                if matches!(t.mode, Mode::Function) {
                    if let Some(sig) = &t.fn_sig {
                        return (*sig.return_type).clone();
                    }
                    // Bound function value without an inferred signature:
                    // opaque. We do NOT fall through to the FnTable path,
                    // because a scope-local binding shadows top-level
                    // definitions and we have no way to refine the local
                    // one. Returning opaque here is the conservative
                    // choice (no false positives, possible false negatives).
                    return RType::unknown();
                } else if !matches!(t.mode, Mode::Opaque) {
                    // R's function/value namespace separation: when a name is
                    // CALLED, R searches the environment chain for a *function*
                    // named `name` and skips non-function bindings. So a local
                    // non-function binding (e.g. `lengths <- lengths(x)`) does
                    // NOT shadow a same-named function in the typeshed or
                    // FnTable at a call site. If such a function exists, fall
                    // through to the resolution below instead of firing RY070.
                    // Only when no function of that name exists anywhere does
                    // calling the non-function value warrant RY070.
                    let has_function_elsewhere = self.has_function_anywhere(&name);
                    if !has_function_elsewhere {
                        // RY070: a non-function value is being called as if it
                        // were a function. R errors at runtime with
                        // "could not find function". Args have already been
                        // inferred above, so we just emit and return opaque
                        // (re-inferring would double-emit arg diagnostics).
                        self.emit(
                            Severity::Error,
                            span,
                            "RY070",
                            format!("`{}` is `{}`, not a function; cannot call it", name, t.mode),
                        );
                        return RType::unknown();
                    }
                    // A function exists elsewhere; fall through to resolve it
                    // (the local non-function binding is ignored at the call
                    // site, matching R).
                }
                // Opaque: fall through; the name might still resolve via
                // the FnTable or typeshed below.
            }
        }

        // Built-in: `c(...)` concatenates and produces the common mode.
        if lookup_name == "c" {
            let result = self.infer_c(args, &arg_types, span);
            if let Some(schema) = build_named_schema(&arg_types, args)
                .filter(|_| args.iter().any(|argument| argument.name.is_some()))
            {
                return result.with_columns(Arc::new(schema));
            }
            return result;
        }
        if lookup_name == "list" {
            return self.infer_list(&arg_types, args, span);
        }
        // `data.frame(...)`: a record constructor. Same column-schema
        // logic as `list(...)`, but the result is classed
        // "data.frame" and column lengths are coerced to a common
        // length (R recycles; for v1 we take the max of the known
        // lengths).
        if lookup_name == "data.frame" {
            if args.len() == 1
                && args[0].name.is_none()
                && let Some(schema) = arg_types[0].columns.clone()
            {
                return RType::new(Mode::List, Length::Known(schema.columns.len()))
                    .with_class(ClassVector::single("data.frame"))
                    .with_columns(schema);
            }
            return self.infer_data_frame(&arg_types, args, span);
        }

        if matches!(lookup_name.as_str(), "t") {
            return arg_types.first().cloned().unwrap_or_else(RType::unknown);
        }

        if matches!(lookup_name.as_str(), "as.data.frame")
            && let Some(input) = arg_types.first()
            && let Some(schema) = input.columns.clone()
        {
            return RType::new(Mode::List, Length::Known(schema.columns.len()))
                .with_class(ClassVector::single("data.frame"))
                .with_columns(schema);
        }

        if let Some(rt) = self.try_s4_dispatch(&lookup_name, &arg_types) {
            return rt;
        }

        if let Some(rt) = arg_types
            .first()
            .and_then(|first| self.user_s3_dispatch_return(&lookup_name, first))
        {
            return rt;
        }

        // S3 dispatch: when a known generic is called with a classed
        // first argument, look up `(generic, class)` in the S3 method
        // table. On a hit, return the method's inferred return type. On
        // a miss with a *known* class, emit RY050. On a miss with an
        // unknown or empty class, fall through (we can't say anything).
        //
        // We model only R's first-element dispatch rule: walking the
        // full class vector (and matching `default`) is a future task.
        // `default` is treated as always-present in the typeshed's S3
        // method table for the common generics, so RY050 never fires
        // for them unless the user explicitly shadows them away.
        //
        // We use the prefix-stripped `lookup_name` so a qualified call
        // like `base::print(x)` still dispatches as `print`.
        if self
            .typeshed
            .globals
            .s3_generics
            .iter()
            .any(|generic| generic == &lookup_name)
        {
            if let Some(rt) = self.try_s3_dispatch(&lookup_name, &arg_types, span) {
                return rt;
            }
            if arg_types
                .first()
                .is_some_and(|argument| argument.class.is_unknown())
            {
                return RType::unknown();
            }
        }

        // Higher-order built-ins (`lapply`, `sapply`, `vapply`, `Map`,
        // `Reduce`, `Filter`, ...): model the callback to infer the
        // result type. Falls through to the typeshed when the name is
        // not one we recognize, so the existing opaque entries for
        // these functions still apply.
        //
        // Before computing the result type, walk the callback body for
        // diagnostics (e.g. RY010 on an unbound name inside the
        // callback). This ensures that `lapply(x, function(i)
        // undefined_var)` still flags the unbound variable, even though
        // the type computation itself is pure.
        //
        // Qualified calls (`base::lapply(...)`) resolve via the
        // stripped `lookup_name`, matching how R treats `::` as a
        // binding selector rather than a different function.
        if resolved_sig
            .as_ref()
            .is_some_and(|signature| signature.higher_order.is_some())
        {
            self.walk_callback_for_diagnostics(&lookup_name, args, &arg_types, scope);
        }
        if let Some(rt) = self.infer_higher_order_call(&lookup_name, args, &arg_types, scope, span)
        {
            return rt;
        }

        // User-defined functions: read from the refined FnTable. We
        // intentionally do NOT refine on demand here - that would risk
        // exponential blowup on deep call chains. The fixpoint loop in
        // `check()` already stabilized the table.
        //
        // Qualified calls look up the stripped name; a user's `utils::
        // helper()` resolves like `helper()`.
        if let Some(f) = self.fn_table.fns.get(&lookup_name) {
            return self.return_slots.get(f.return_slot);
        }

        // Literal-arg inference for `vector`, `rep`, `seq`, `seq.int`.
        // These have typeshed entries that conservatively return
        // `Length::Unknown`; when the relevant arguments are literals
        // we can pin the result length exactly. We place this AFTER the
        // FnTable lookup so a user-defined `rep`/`seq` still wins, and
        // BEFORE the typeshed so the precise length is preferred over
        // the conservative `x_times` / `unknown` spec.
        if lookup_name == "vector" {
            return self.infer_vector(args);
        }
        if lookup_name == "rep" {
            return self.infer_rep(args, &arg_types, span);
        }
        if lookup_name == "seq" || lookup_name == "seq.int" {
            return self.infer_seq(args, &arg_types, span);
        }

        // Look up in the typeshed. A qualified call (`pkg::fun`) is
        // resolved against `load_package(pkg)`; an unqualified call
        // falls back from base to loaded packages (reverse load order).
        // We pass the full `name` (with any `pkg::` prefix) so the
        // resolver can dispatch on qualification.
        if let Some(sig) = resolved_sig {
            return self.apply_sig(&lookup_name, &sig, &arg_types, args, span);
        }

        // Unknown function: opaque.
        RType::unknown()
    }

    pub(crate) fn try_s4_dispatch(&self, generic: &str, arg_types: &[RType]) -> Option<RType> {
        let class = arg_types.first()?.class.first()?;
        let slot = self
            .fn_table
            .s4_methods
            .get(&(generic.to_string(), class.to_string()))?;
        Some(self.return_slots.get(*slot))
    }

    // Infer the type of `structure(x, class = "...")`. We model only
    // the literal class forms; everything else returns the first
    // argument's type with `ClassVector::unknown()` (so we neither lie
    // about a class nor spuriously trigger RY050).
    //
    // The base value's column schema is preserved: `RType::with_class`
    // is `RType { class, ..self }`, so a `structure(list(a = 1L),
    // class = "foo")` call yields a value whose columns are still
    // `[("a", integer<1>)]` and whose class is `["foo"]`. This lets
    // `$a` resolve correctly on user-defined classes built on top of
    // a list-shaped payload.
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
        let matches = match_arguments(&params, args);
        let mut arg_types = Vec::with_capacity(args.len());
        for (index, argument) in args.iter().enumerate() {
            let parameter = matches.param_for_arg[index].and_then(|index| params.get(index));
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
            arg_types.push(if injects_fixed_names {
                self.infer_injected_expr(&argument.value, &mut child)
            } else {
                self.infer(&argument.value, &mut child)
            });
        }
        self.check_typeshed_call_arguments(lookup_name, &signature, args, &arg_types, span);
        Some(self.apply_sig(lookup_name, &signature, &arg_types, args, span))
    }

    fn infer_injected_expr(&mut self, expr: &Expr, scope: &mut Scope) -> RType {
        match expr {
            Expr::Function { params, body, .. } => {
                let mut inner = scope.clone();
                for parameter in params {
                    inner.insert(parameter.name.clone(), RType::unknown());
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

    pub(crate) fn infer_structure_call(
        &mut self,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        // The base value is the first positional argument (or the
        // `x = ...` named argument). The first such positional-or-`x`
        // arg wins; later ones are inferred for diagnostics only.
        let mut base_type = RType::unknown();
        let mut class_expr: Option<&Expr> = None;
        for a in args {
            if matches!(a.name.as_deref(), Some("class")) {
                class_expr = Some(&a.value);
                continue;
            }
            let is_base = matches!(a.name.as_deref(), None | Some("x"))
                && matches!(base_type.mode, Mode::Opaque);
            if is_base {
                base_type = self.infer(&a.value, scope);
            } else {
                let _ = self.infer(&a.value, scope);
            }
        }
        if let Some(ce) = class_expr {
            match parse_class_literal(ce) {
                ClassLiteral::Single(name) => {
                    return base_type.with_class(ClassVector::single(&name));
                }
                ClassLiteral::Multi(names) => {
                    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    return base_type.with_class(ClassVector::from_slice(&refs));
                }
                ClassLiteral::Unknown => {
                    // Class is dynamic; keep base type but mark class as
                    // undetermined so RY050 stays quiet.
                    return base_type.with_class(ClassVector::unknown());
                }
            }
        }
        let _ = span;
        base_type
    }

    // Handle R's Non-Standard Evaluation verbs (`subset`, `with`,
    // `within`, `transform`). These evaluate their expression
    // arguments in an augmented scope where the data frame's columns
    // are bound as names, so `subset(df, cyl == 4)` resolves `cyl`
    // against `df`'s column schema rather than the enclosing scope.
    //
    // Returns `Some(t)` when the call was recognized as an NSE verb
    // (the caller uses `t` verbatim and skips the regular arg-inference
    // path). Returns `None` for non-NSE names so `infer_call` falls
    // through to the regular path.
    //
    // Behavior when the first arg has no column schema: we cannot
    // enumerate the columns, so the expression arguments cannot be
    // type-checked meaningfully. We still infer them against the bare
    // scope (no column augmentation) so any genuinely unbound name in
    // the expression still emits RY010; this mirrors the conservative
    // approach for unknown data throughout the checker.
    //
    // The augmented scope is local to this call: column bindings must
    // NOT leak back into the enclosing scope (we operate on a clone).
}

fn is_user_infix_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with('%') && name.ends_with('%')
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
