use super::*;
use crate::higher_order::s3_group_generic;

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
            // R permits a string literal as a call head, e.g. `"[<-"(...)`.
            // Treat it exactly like the corresponding identifier so it takes
            // the normal user-function, typeshed, S3, and higher-order paths.
            Expr::Ident { name, .. } | Expr::String(name, _) => name.clone(),
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
        let lookup_name = if is_user_infix_name(&semantic_name) {
            // `%::%` is an infix operator, not a namespace-qualified call.
            // Do not split the `::` embedded in its operator spelling.
            semantic_name.clone()
        } else {
            semantic_name
                .rsplit_once("::")
                .map(|(_, n)| n.to_string())
                .unwrap_or_else(|| semantic_name.clone())
        };

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

        // Unknown custom infix operators are commonly small DSLs that quote
        // both operands with `match.call()` or `substitute()`.  Treating
        // their operands as ordinary R expressions produces false positives
        // for DSL-only names and operations (for example lambda.r
        // declarations and plyr's formula-like helpers). They are language
        // objects, so infer them only to preserve traversal invariants and
        // never emit a diagnostic from inside either operand.
        //
        // A user-defined operator or a typeshed-known one remains an ordinary
        // evaluated call: `has_function_anywhere` covers both sources.
        // `.()` is the analogous quoting helper used by plyr/data.table.
        let custom_infix_is_known = self.has_function_anywhere(&semantic_name)
            || self
                .fn_table
                .fns
                .keys()
                .any(|name| semantic_argument_name(name) == semantic_name);
        if (is_user_infix_name(&semantic_name) || semantic_name == ".") && !custom_infix_is_known {
            let mut quoted_scope = scope.clone();
            for argument in args {
                self.infer_discarding(&argument.value, &mut quoted_scope);
            }
            return RType::unknown();
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
        if matches!(lookup_name.as_str(), "length" | "nchar")
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

        // Numeric math functions coerce logical comparisons to 0/1, which is
        // almost always a misplaced parenthesis (`abs(x > y)` rather than
        // `abs(x) > y`). Extra parentheses do not change the parsed argument,
        // so deliberately parenthesized comparisons remain visible here.
        if matches!(
            lookup_name.as_str(),
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
        ) && let Some(Expr::BinOp { op, span, .. }) = args.first().map(|arg| &arg.value)
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
                "RY100",
                "comparison directly inside a numeric math function is usually a parenthesization mistake; compare the math result instead",
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

        // `on.exit(expr)` evaluates `expr` when the enclosing function
        // returns, rather than where it is registered.  Names assigned later
        // in that body therefore exist by the time this expression runs.
        // Seed only those statically assigned names and still infer the
        // expression normally, so genuinely unbound names retain RY010.
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
            return RType::new(Mode::Null, Length::Zero);
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
            return RType::new(Mode::Character, Length::Unknown);
        }

        if semantic_name == "load" {
            scope.mark_search_path_unknown();
            for argument in args {
                let _ = self.infer(&argument.value, scope);
            }
            if let Some(bindings) = self.load_bindings.get(&span.start).cloned() {
                for binding in bindings {
                    if binding == crate::SERIALIZED_BINDINGS_UNENUMERABLE {
                        // An oversized workspace may introduce any binding.
                        scope.mark_search_path_unknown();
                    } else {
                        scope.insert(binding, RType::unknown());
                    }
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
            if lookup_name == "alist" {
                return RType::new(Mode::List, Length::Unknown);
            }
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
        let user_function = self.fn_table.fns.get(&lookup_name).cloned().or_else(|| {
            self.fn_table
                .fns
                .iter()
                .find(|(name, _)| semantic_argument_name(name) == lookup_name)
                .map(|(_, function)| function.clone())
        });
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
            let is_quoting = user_argument_matches
                .as_ref()
                .and_then(|matches| matches.param_for_arg[index].or(matches.dots))
                .and_then(|parameter| user_function.as_ref()?.params.get(parameter))
                .is_some_and(|parameter| parameter.quoting);
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
            if supplied_data_mask_source
                .as_ref()
                .is_some_and(|(source_index, _)| *source_index == index)
            {
                arg_types.push(
                    supplied_data_mask_source
                        .as_ref()
                        .expect("checked data-mask source")
                        .1
                        .clone(),
                );
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
        if !name.contains("::")
            && let Some(mut target) = standalone_check_target(&lookup_name)
            && user_function.as_ref().is_none_or(|function| {
                ["arg", "call"].into_iter().all(|required| {
                    function
                        .params
                        .iter()
                        .any(|parameter| parameter.name == required)
                })
            })
            && let Some(Expr::Ident { name: var, .. }) = args.first().map(|a| &a.value)
        {
            // A non-literal opt-in is treated like TRUE: weakening the fact
            // avoids excluding a value the checker may accept at runtime.
            if args.iter().any(|argument| {
                argument.name.as_deref() == Some("allow_null")
                    && !matches!(argument.value, Expr::Logical(false, _))
            }) {
                target = target.join(RType::new(Mode::Null, Length::Zero));
            }
            if args.iter().any(|argument| {
                argument.name.as_deref() == Some("allow_na")
                    && !matches!(argument.value, Expr::Logical(false, _))
            }) {
                target = target.join(RType::scalar(Mode::Logical));
            }
            scope.insert(var.clone(), target);
            return RType::new(Mode::Null, Length::Zero);
        }

        let assertion_predicates =
            name == "stopifnot" || name == "assert_that" || name == "assertthat::assert_that";
        if assertion_predicates {
            for argument in args {
                if name.ends_with("assert_that") && argument.name.as_deref() == Some("msg") {
                    continue;
                }
                let narrowing = extract_type_narrowing(&argument.value);
                let (positive_scope, _, _) = apply_narrowing(scope, &narrowing, false);
                *scope = positive_scope;
            }
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
                } else if let Some(result) = self.callable_function_union(t, args, &arg_types) {
                    return result;
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
            && !schema.is_empty()
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
        // Dispatch walks the complete class vector, then considers the
        // direct generic and its Math/Summary group fallback.
        //
        // We use the prefix-stripped `lookup_name` so a qualified call
        // like `base::print(x)` still dispatches as `print`.
        if self
            .typeshed
            .globals
            .s3_generics
            .iter()
            .any(|generic| generic == &lookup_name)
            || s3_group_generic(&lookup_name).is_some()
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
        if let Some(function) = user_function.as_ref() {
            return self.return_slots.get(function.return_slot);
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
            let quoted_expression = matches!(
                argument_eval_mode(&signature, args, index),
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
        Some(self.apply_sig(lookup_name, &signature, &arg_types, args, span))
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

        let mut returns = members.iter().map(|member| {
            member
                .fn_sig
                .as_ref()
                .map(|signature| (*signature.return_type).clone())
                .unwrap_or_else(RType::unknown)
        });
        let first = returns.next().unwrap_or_else(RType::unknown);
        Some(returns.fold(first, RType::join))
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
