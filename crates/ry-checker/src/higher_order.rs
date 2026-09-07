use super::*;
use crate::infer::*;

fn matched_argument_type<'a>(
    arg_types: &'a [RType],
    argument_match: &ArgumentMatch,
    formal_index: usize,
) -> Option<&'a RType> {
    argument_match
        .arg_for_param(formal_index)
        .and_then(|index| arg_types.get(index))
}

fn argument_bound_to_formal<'a>(
    args: &'a [Arg],
    argument_match: &ArgumentMatch,
    formal_index: usize,
) -> Option<&'a Arg> {
    argument_match
        .arg_for_param(formal_index)
        .and_then(|index| args.get(index))
}

fn simplify_control(
    base_identity: Option<bool>,
    forwarded_dots: bool,
    params: &[ParamSpec],
    args: &[Arg],
    argument_match: &ArgumentMatch,
) -> Option<bool> {
    match base_identity {
        Some(true) => {}
        Some(false) => return Some(true),
        // An unresolved bare name may be supplied by an attached package;
        // its control must not be treated as the base contract by default.
        None => return None,
    }
    let control = params
        .iter()
        .position(|param| matches!(param.name.as_str(), "simplify" | "SIMPLIFY"));
    let Some(control) = control else {
        return Some(true);
    };
    let Some(argument) = argument_bound_to_formal(args, argument_match, control) else {
        // Forwarded dots can supply an omitted simplify control, so the
        // default cannot establish the base function's enabled contract.
        if forwarded_dots {
            return None;
        }
        return Some(true);
    };
    match &argument.value {
        Expr::Logical(value, _) => Some(*value),
        _ => None,
    }
}

fn definitely_nonempty(length: Length) -> bool {
    matches!(length, Length::One) || matches!(length, Length::Known(n) if n > 0)
}

fn result_may_be_empty(
    spec: &HigherOrderSpec,
    arg_types: &[RType],
    argument_match: &ArgumentMatch,
    inputs: &[RType],
) -> bool {
    if spec.callback_args == [CallbackArg::ElementsAfterCallback] {
        return inputs.is_empty()
            || inputs
                .iter()
                .any(|input| !definitely_nonempty(input.length));
    }
    let Some(length_arg) = spec.result.length_arg else {
        return false;
    };
    !matched_argument_type(arg_types, argument_match, length_arg)
        .is_some_and(|input| definitely_nonempty(input.length))
}

/// With a `...` formal, every unmatched actual is part of dots (one
/// ordinary R argument match drives callback, source, length, and
/// template lookup throughout the higher-order path).
fn arguments_bound_to_dots<'a>(
    arg_types: &'a [RType],
    argument_match: &'a ArgumentMatch,
) -> impl Iterator<Item = &'a RType> {
    arg_types
        .iter()
        .enumerate()
        .filter_map(|(index, argument_type)| {
            // Preserve actual call order because Map/mapply pass that
            // order on to the callback.
            (argument_match.dots.is_some()
                && argument_match
                    .param_for_arg
                    .get(index)
                    .is_some_and(Option::is_none))
            .then_some(argument_type)
        })
}

impl Checker {
    pub(crate) fn infer_higher_order_call(
        &mut self,
        name: &str,
        signature: &FunctionSig,
        args: &[Arg],
        arg_types: &[RType],
        scope: &mut Scope,
        span: Span,
    ) -> Option<RType> {
        let spec = signature.higher_order.as_ref()?;
        // The simplification controls have this meaning only for the base
        // apply contracts. Resolve the callee before callback
        // traversal, which can extend the lexical scope with callback data.
        let base_simplification = if !matches!(
            crate::semantic_lists::bare_name(name),
            "sapply" | "mapply" | "tapply"
        ) {
            Some(false)
        } else if !self.user_stubs.contains_key("base") && self.resolves_to_base(name, scope) {
            Some(true)
        } else if name.contains("::") || self.user_stubs.contains_key("base") {
            Some(false)
        } else {
            None
        };
        let forwarded_dots = args.iter().any(|argument| self.is_forwarded_dots(argument));
        self.walk_callback_for_diagnostics(signature, args, arg_types, scope);
        // A fold's initializer describes only the first invocation. Later
        // accumulators are callback results, and zero iterations return the
        // initializer/input directly. Neither one invocation nor an input
        // element establishes the final result (including accumulated output).
        if spec
            .callback_args
            .contains(&CallbackArg::AccumulatorAndElement)
        {
            return Some(RType::unknown());
        }
        let argument_match = match_params(&signature.params, args);
        let simplify = simplify_control(
            base_simplification,
            forwarded_dots,
            &signature.params,
            args,
            &argument_match,
        );
        let declared_length = match &signature.return_ {
            ReturnSpec::Concrete(ty) => json_length_to_length(JsonLength::parse(&ty.length)),
            ReturnSpec::Slot(_) => Length::Unknown,
        };
        Some(self.infer_ho_result(
            name,
            spec,
            declared_length,
            args,
            arg_types,
            &argument_match,
            scope,
            span,
            simplify,
        ))
    }

    /// Per-builtin result-type computation. Used by both pass 2 (pure,
    /// via `infer_discarding`) and pass 3 (diagnostic-emitting). This is
    /// the pass-3 entry point: it calls `self.infer` on data
    /// arguments (which may emit RY010 etc.) before computing the
    /// element type.
    #[allow(clippy::too_many_arguments)]
    fn infer_ho_result(
        &mut self,
        name: &str,
        spec: &HigherOrderSpec,
        declared_length: Length,
        args: &[Arg],
        arg_types: &[RType],
        argument_match: &ArgumentMatch,
        scope: &Scope,
        span: Span,
        simplify: Option<bool>,
    ) -> RType {
        let inputs = self.higher_order_input_types(spec, arg_types, argument_match);
        let callback_runs = !inputs.is_empty()
            && !inputs.iter().any(|ty| {
                matches!(ty.length, Length::Zero | Length::Known(0)) || ty.mode == Mode::Function
            });
        let callback_types = inputs.iter().map(RType::element).collect::<Vec<_>>();
        let callback = argument_bound_to_formal(args, argument_match, spec.callback_position)
            .map(|argument| &argument.value);
        let callback_return = callback
            .filter(|_| callback_runs)
            .and_then(|callback| self.callback_return_type(callback, &callback_types, scope));
        if let Some(target) = spec
            .callback_return_mode
            .as_deref()
            .and_then(concrete_json_mode)
            && let Some(actual) = &callback_return
            && (!modes_compatible(&actual.mode, &target)
                || matches!(actual.length, Length::Zero | Length::Known(0 | 2..)))
        {
            let bare_name = crate::semantic_lists::bare_name(name);
            self.emit(
                Severity::Error,
                span,
                "RY080",
                format!("`{bare_name}` requires a scalar `{target}` callback result, but the callback returns `{actual}`; R rejects this result"),
            );
        }
        match spec.result.kind {
            HigherOrderResultKind::ListOfCallbackReturn => {
                let length = spec
                    .result
                    .length_arg
                    .and_then(|i| matched_argument_type(arg_types, argument_match, i))
                    .map(|ty| ty.length)
                    .unwrap_or(Length::Unknown);
                let mut result = RType::new(Mode::List, length);
                if spec.result.include_callback_schema {
                    if let Some(element_type) = callback_return
                        && !matches!(element_type.mode, Mode::Opaque)
                    {
                        let n = match length {
                            Length::Known(n) if n > 0 => n,
                            _ => 1,
                        };
                        result = result.with_columns(Arc::new(ColumnSchema {
                            columns: (0..n)
                                .map(|i| (format!("[[{}]]", i + 1), element_type.clone()))
                                .collect(),
                            complete: matches!(length, Length::Known(_)),
                            locally_constructed: false,
                        }));
                    }
                }
                result
            }
            HigherOrderResultKind::VectorOf => {
                let mode = higher_order_mode(spec.result.mode.as_deref());
                if matches!(mode, Mode::Opaque) {
                    return RType::unknown();
                }
                let length = spec
                    .result
                    .length_arg
                    .and_then(|i| matched_argument_type(arg_types, argument_match, i))
                    .map(|ty| ty.length)
                    .unwrap_or(declared_length);
                RType::new(mode, length)
            }
            HigherOrderResultKind::SameAsArg0 => {
                let mut result = matched_argument_type(
                    arg_types,
                    argument_match,
                    spec.result.source_arg.unwrap_or(0),
                )
                .cloned()
                .unwrap_or_else(RType::unknown);
                if spec.result.unknown_length {
                    result.length = Length::Unknown;
                }
                result
            }
            HigherOrderResultKind::CallbackReturn => {
                if let Some(index) = spec.result.source_arg {
                    matched_argument_type(arg_types, argument_match, index)
                        .map(RType::element)
                        .unwrap_or_else(RType::unknown)
                } else {
                    callback_return.unwrap_or_else(RType::unknown)
                }
            }
            HigherOrderResultKind::FirstArg => matched_argument_type(
                arg_types,
                argument_match,
                spec.result.source_arg.unwrap_or(0),
            )
            .cloned()
            .unwrap_or_else(RType::unknown),
            HigherOrderResultKind::Simplify => {
                if spec.callback_args == [CallbackArg::Unknown] {
                    return Self::ho_rapply(args, arg_types, argument_match);
                }
                if simplify != Some(true) {
                    return if simplify == Some(false) {
                        RType::new(Mode::List, Length::Unknown)
                    } else {
                        RType::unknown()
                    };
                }
                if result_may_be_empty(spec, arg_types, argument_match, &inputs) {
                    return RType::unknown();
                }
                match callback_return {
                    Some(ty)
                        if matches!(ty.length, Length::One)
                            && !matches!(ty.mode, Mode::List | Mode::Opaque | Mode::Union) =>
                    {
                        let length = spec
                            .result
                            .length_arg
                            .and_then(|i| matched_argument_type(arg_types, argument_match, i))
                            .map(|ty| ty.length)
                            .unwrap_or(Length::Unknown);
                        RType::new(ty.mode, length)
                    }
                    _ => RType::unknown(),
                }
            }
            HigherOrderResultKind::FunValueTemplate => {
                let template = spec
                    .result
                    .template_position
                    .and_then(|i| matched_argument_type(arg_types, argument_match, i))
                    .cloned()
                    .unwrap_or_else(RType::unknown);
                let mode = if matches!(template.mode, Mode::Union) {
                    Mode::Opaque
                } else {
                    template.mode
                };
                let length = if matches!(template.length, Length::One) {
                    spec.result
                        .length_arg
                        .and_then(|i| matched_argument_type(arg_types, argument_match, i))
                        .map(|ty| ty.length)
                        .unwrap_or(Length::Unknown)
                } else {
                    Length::Unknown
                };
                RType::new(mode, length)
            }
            HigherOrderResultKind::CallbackIdentity => {
                self.ho_callback_identity(spec, args, argument_match, scope)
            }
        }
    }

    fn higher_order_input_types(
        &self,
        spec: &HigherOrderSpec,
        arg_types: &[RType],
        argument_match: &ArgumentMatch,
    ) -> Vec<RType> {
        let mut types = Vec::new();
        for callback_arg in &spec.callback_args {
            match callback_arg {
                CallbackArg::ElementOfArg0 => types.push(
                    matched_argument_type(arg_types, argument_match, 0)
                        .cloned()
                        .unwrap_or_else(RType::unknown),
                ),
                CallbackArg::ElementOfArg1 => types.push(
                    matched_argument_type(arg_types, argument_match, 1)
                        .cloned()
                        .unwrap_or_else(RType::unknown),
                ),
                CallbackArg::ElementsOfArg0 => {
                    if matched_argument_type(arg_types, argument_match, 0)
                        .is_some_and(|ty| matches!(ty.length, Length::Zero | Length::Known(0)))
                    {
                        continue;
                    }
                    if let Some(schema) = matched_argument_type(arg_types, argument_match, 0)
                        .and_then(|ty| ty.columns.as_ref())
                        .filter(|schema| schema.complete)
                    {
                        types.extend(schema.columns.iter().map(|(_, ty)| ty.clone()));
                    } else {
                        types.push(RType::unknown());
                    }
                }
                CallbackArg::Unknown => types.push(RType::unknown()),
                CallbackArg::AccumulatorAndElement => {
                    types.extend([RType::unknown(), RType::unknown()]);
                }
                CallbackArg::ElementsAfterCallback => {
                    types.extend(arguments_bound_to_dots(arg_types, argument_match).cloned())
                }
            }
        }
        types
    }

    /// If `expr` is a `purrr::in_parallel(.f)` / `in_parallel(.f)` call
    /// wrapping a function literal or name, return the inner `.f`.
    /// `in_parallel` is type-transparent (purrr >= 1.1.0), so callers
    /// that infer a callback's return type or walk its body should look
    /// through it. Returns the original expression unchanged otherwise.
    pub(crate) fn unwrap_callback_identity<'a>(&self, expr: &'a Expr) -> &'a Expr {
        if let Expr::Call { func, args, .. } = expr {
            if let Expr::Ident { name, .. } = func.as_ref() {
                let is_identity = self
                    .resolve_typeshed_sig(name)
                    .and_then(|sig| sig.higher_order)
                    .is_some_and(|spec| {
                        matches!(spec.result.kind, HigherOrderResultKind::CallbackIdentity)
                    });
                if is_identity {
                    if let Some(first) = args.first() {
                        return &first.value;
                    }
                }
            }
        }
        expr
    }

    /// `purrr::in_parallel(.f)`: a type-transparent wrapper (purrr >=
    /// 1.1.0). Returns `.f` unchanged so `map(sims, in_parallel(f))`
    /// checks identically to `map(sims, f)`. `.f` may be a function
    /// literal (returned as a function value) or a name (resolved via
    /// the scope/typeshed to a function value).
    pub(crate) fn ho_callback_identity(
        &mut self,
        spec: &HigherOrderSpec,
        args: &[Arg],
        argument_match: &ArgumentMatch,
        scope: &Scope,
    ) -> RType {
        let cb = match argument_bound_to_formal(args, argument_match, spec.callback_position) {
            Some(argument) => &argument.value,
            None => return RType::unknown(),
        };
        match cb {
            Expr::Function { .. } => RType::scalar(Mode::Function),
            Expr::Ident { name, .. } => {
                // A bound function value resolves to its type; an
                // unbound name that names a typeshed function resolves
                // to a function value; anything else is treated as a
                // function (in_parallel is transparent, and an unknown
                // callback is most plausibly a function from a package
                // we don't model).
                scope
                    .get(name)
                    .cloned()
                    .unwrap_or(RType::scalar(Mode::Function))
            }
            _ => RType::unknown(),
        }
    }

    /// Recursive simplification can return NULL, an atomic vector, or a list.
    /// Only literal list/replace controls establish a retained outer shape.
    fn ho_rapply(args: &[Arg], arg_types: &[RType], argument_match: &ArgumentMatch) -> RType {
        let Some(Expr::String(how, _)) =
            argument_bound_to_formal(args, argument_match, 4).map(|argument| &argument.value)
        else {
            return RType::unknown();
        };
        // R's match.arg() accepts unique nonempty prefixes of these modes.
        if how.is_empty() {
            return RType::unknown();
        }
        let input = matched_argument_type(arg_types, argument_match, 0);
        if "list".starts_with(how.as_str()) {
            return RType::new(Mode::List, input.map_or(Length::Unknown, |ty| ty.length));
        }
        if "replace".starts_with(how.as_str()) {
            if let Some(input) = input.filter(|ty| ty.mode == Mode::List) {
                return RType::new(Mode::List, input.length).with_class(input.class.clone());
            }
        }
        // replace also accepts expressions, whose runtime mode is not modeled.
        // unlist depends on recursive leaves, filtering, defaults, and callbacks.
        RType::unknown()
    }

    /// Infer the return type of a single callback invocation, given the
    /// argument types the higher-order function will pass to it.
    ///
    /// Covers four callback forms:
    ///   * `Expr::Function { params, body }` (anonymous literal): walk
    ///     the body with a scope containing the params bound to the
    ///     element types, collecting returns. Bounded by
    ///     `MAX_CLOSURE_DEPTH`.
    ///   * `Expr::Ident { name }` bound in scope to a
    ///     `Mode::Function` value with `fn_sig`: use the signature's
    ///     return type.
    ///   * `Expr::Ident { name }` referring to a user-fn: read its
    ///     refined return slot.
    ///   * `Expr::Ident { name }` referring to a typeshed function:
    ///     apply its signature with the element types as arguments.
    ///
    /// Returns `None` when the callback form is not recognized or the
    /// return type is opaque (caller falls back to the conservative
    /// per-builtin default).
    pub(crate) fn callback_return_type(
        &mut self,
        callback: &Expr,
        call_arg_types: &[RType],
        scope: &Scope,
    ) -> Option<RType> {
        // Look through a `purrr::in_parallel(.f)` / `in_parallel(.f)`
        // wrapper: it is type-transparent, so the callback's return is
        // the inner function's return.
        let callback = self.unwrap_callback_identity(callback);
        match callback {
            Expr::Function { params, body, .. } => {
                self.callback_literal_return(params, body, call_arg_types, scope, 0)
            }
            Expr::Ident { name, .. } => {
                // Strip any `pkg::` namespace prefix so a qualified
                // callback name (`base::sqrt` passed to `sapply`)
                // resolves against the same entries as the bare name.
                // `bare_name` handles both `::` and `:::`.
                let lookup_name = crate::semantic_lists::bare_name(name);
                // Bound closure value in scope?
                if let Some(t) = scope.get(lookup_name) {
                    if matches!(t.mode, Mode::Function) {
                        if let Some(sig) = &t.fn_sig {
                            return Some((*sig.return_type).clone());
                        }
                        return None;
                    }
                    if matches!(t.mode, Mode::Union)
                        && let Some(members) = &t.members
                        && !members.is_empty()
                        && members.iter().all(|member| member.mode == Mode::Function)
                    {
                        let returns = members.iter().map(|member| {
                            member
                                .fn_sig
                                .as_ref()
                                .map(|signature| (*signature.return_type).clone())
                                .unwrap_or_else(RType::unknown)
                        });
                        return Some(join_all(returns));
                    }
                }
                // User-defined function in the FnTable?
                if let Some(f) = self.fn_table.fns.get(lookup_name) {
                    let rt = self.read_return_slot(f.return_slot);
                    if !matches!(rt.mode, Mode::Opaque) {
                        return Some(rt);
                    }
                    return None;
                }
                // Typeshed function?
                if let Some(sig) = self.resolve_typeshed_sig(name) {
                    return Some(self.apply_sig(&sig, call_arg_types, &[]));
                }
                None
            }
            _ => None,
        }
    }

    /// Walk an anonymous function literal's body to infer its return
    /// type, given the argument types the caller will pass: the shared
    /// [`Checker::walk_literal_returns`] walk with params bound from
    /// the call-site argument types instead of declared defaults.
    /// Used by `callback_return_type` for the inline-literal case.
    pub(crate) fn callback_literal_return(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        call_arg_types: &[RType],
        captured_scope: &Scope,
        depth: usize,
    ) -> Option<RType> {
        if depth >= MAX_CLOSURE_DEPTH {
            return None;
        }
        self.walk_literal_returns(body, captured_scope, depth, |scope| {
            for (i, p) in params.iter().enumerate() {
                let t = call_arg_types.get(i).cloned().unwrap_or(RType::unknown());
                scope.insert(p.name.clone(), t);
            }
        })
    }

    fn fold_callback_inputs(
        signature: &FunctionSig,
        args: &[Arg],
        arg_types: &[RType],
        argument_match: &ArgumentMatch,
    ) -> Vec<RType> {
        let unknown = || vec![RType::unknown(), RType::unknown()];
        // Dots can supply controls or shift positional matching.
        if args
            .iter()
            .any(|arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "..."))
        {
            return unknown();
        }
        let formal = |name| signature.params.iter().position(|param| param.name == name);
        let actual =
            |name| formal(name).and_then(|i| argument_bound_to_formal(args, argument_match, i));
        let (data, init, right) = if formal("right").is_some() {
            let right = match actual("right").map(|arg| &arg.value) {
                None | Some(Expr::Missing(_)) => Some(false),
                Some(Expr::Logical(value, _)) => Some(*value),
                _ => None,
            };
            (formal("x"), actual("init"), right)
        } else if formal(".dir").is_some() {
            let right = match actual(".dir").map(|arg| &arg.value) {
                None | Some(Expr::Missing(_)) => Some(false),
                Some(Expr::String(value, _)) if value == "forward" => Some(false),
                Some(Expr::String(value, _)) if value == "backward" => Some(true),
                _ => None,
            };
            (formal(".x"), actual(".init"), right)
        } else {
            return unknown();
        };
        let Some(input) = data.and_then(|i| matched_argument_type(arg_types, argument_match, i))
        else {
            return unknown();
        };
        if input.class.is_unknown() || input.class.has_known_class() {
            return unknown();
        }
        let init_missing = init.is_none_or(|arg| matches!(arg.value, Expr::Missing(_)));
        if matches!(input.length, Length::Zero | Length::Known(0))
            || (init_missing && matches!(input.length, Length::One | Length::Known(1)))
        {
            return Vec::new();
        }
        // Keep the element operand precise, but never reuse the initializer
        // or input element as the loop-carried accumulator's type.
        match right {
            Some(false) => vec![RType::unknown(), input.clone()],
            Some(true) => vec![input.clone(), RType::unknown()],
            None => unknown(),
        }
    }

    /// Walk the callback body of a higher-order function call for
    /// diagnostics (RY010 unbound variables, RY040 type errors, etc.).
    /// Called from pass 3 (`infer_call`) before the type-computation
    /// path, which is pure. This ensures that errors inside the
    /// callback body are surfaced even though the type computation
    /// itself doesn't emit diagnostics.
    ///
    /// For each callback (inline anonymous function literal), we build
    /// a scope with the callback's params bound to the element types
    /// the higher-order function will pass, then walk the body's
    /// statements via `check_stmt` (which emits diagnostics). Named
    /// callbacks (user-fn, typeshed) don't need this: their bodies are
    /// walked during the user-fn fixpoint or are built-in.
    fn walk_callback_for_diagnostics(
        &mut self,
        signature: &FunctionSig,
        args: &[Arg],
        arg_types: &[RType],
        scope: &mut Scope,
    ) {
        let spec = match signature.higher_order.as_ref() {
            Some(spec) => spec,
            None => return,
        };
        if matches!(spec.result.kind, HigherOrderResultKind::CallbackIdentity) {
            return;
        }
        let argument_match = match_params(&signature.params, args);
        let inputs = if spec.callback_args == [CallbackArg::AccumulatorAndElement] {
            Self::fold_callback_inputs(signature, args, arg_types, &argument_match)
        } else {
            self.higher_order_input_types(spec, arg_types, &argument_match)
        };
        if inputs.is_empty()
            || inputs.iter().any(|ty| {
                matches!(ty.length, Length::Zero | Length::Known(0)) || ty.mode == Mode::Function
            })
        {
            return;
        }
        let elem_types = inputs.iter().map(RType::element).collect::<Vec<_>>();
        let cb = match argument_bound_to_formal(args, &argument_match, spec.callback_position) {
            Some(argument) => &argument.value,
            None => return,
        };
        // Look through a `purrr::in_parallel(.f)` wrapper so the inner
        // function's body is walked (in_parallel is type-transparent).
        let cb = self.unwrap_callback_identity(cb);
        if let Expr::Function { params, body, .. } = cb {
            let mut fn_scope = scope.independent_execution_scope();
            for (i, p) in params.iter().enumerate() {
                let t = elem_types.get(i).cloned().unwrap_or(RType::unknown());
                fn_scope.insert(p.name.clone(), t);
            }
            for s in body {
                self.check_stmt(s, &mut fn_scope);
            }
        }
    }

    /// Try S3 dispatch for a known generic. Returns `Some(rt)` if a
    /// method was found or a diagnostic was emitted (the caller should
    /// use the returned type directly). Returns `None` only when the
    /// caller should fall through to other resolution paths. The
    /// method-source ladder is shared with operator dispatch
    /// (`infer/binop.rs`); the miss tail is not.
    ///
    /// Design note: we deliberately return `Option<RType>` rather than
    /// `RType` because the caller (`infer_call`) may still want to
    /// consult the user-fn table or the typeshed for non-S3 forms (e.g.
    /// when the first arg is opaque).
    pub(crate) fn try_s3_dispatch(
        &mut self,
        generic: &str,
        arg_types: &[RType],
        span: Span,
    ) -> Option<RType> {
        let first = arg_types.first().cloned()?;
        let cv = first.class;
        if !cv.has_known_class() {
            // No known class (either empty or unknown): nothing for S3
            // dispatch to do. The caller will try user-fn/typeshed
            // resolution against the bare name.
            return None;
        }
        let generics = std::iter::once(generic)
            .chain(s3_group_generic(generic))
            .collect::<Vec<_>>();
        // R tries each class in order. For every class, the specific generic
        // wins over its group generic (e.g. `abs.foo` before `Math.foo`).
        for class in cv.names.iter().take(cv.len as usize).flatten() {
            if &**class == "default" {
                continue;
            }
            for candidate in &generics {
                match self.s3_lookup_method(candidate, class) {
                    Some(S3MethodSource::Registered) => return Some(RType::unknown()),
                    Some(S3MethodSource::Project(slot)) => {
                        return Some(self.s3_specific_or_group_return(*candidate == generic, slot));
                    }
                    Some(S3MethodSource::Stub(sig)) => {
                        return Some(self.apply_sig(&sig, arg_types, &[]));
                    }
                    None => {}
                }
            }
        }
        self.s3_dispatch_miss(generic, &generics, &cv, span)
    }

    /// One `(generic, class)` rung of the method-source ladder:
    /// external registrations, the project fn table, the base typeshed,
    /// then package typesheds. Shared with the operator dispatch in
    /// `infer/binop.rs` so the two paths cannot disagree about which
    /// methods exist.
    pub(crate) fn s3_lookup_method(&self, generic: &str, class: &str) -> Option<S3MethodSource> {
        let key = (generic.to_string(), class.to_string());
        if self.external_s3_methods.contains(&key) {
            return Some(S3MethodSource::Registered);
        }
        if let Some(slot) = self.fn_table.s3_methods.get(&key) {
            return Some(S3MethodSource::Project(*slot));
        }
        if let Some(sig) = self.typeshed.s3_methods.get(&key) {
            return Some(S3MethodSource::Stub(Box::new(sig.clone())));
        }
        self.available_package_names()
            .find_map(|pkg| {
                self.package_typeshed(pkg)
                    .and_then(|typeshed| typeshed.s3_methods.get(&key))
                    .cloned()
            })
            .map(|sig| S3MethodSource::Stub(Box::new(sig)))
    }

    /// A project method's return: a specific method (`abs.foo` called
    /// as `abs`) has an inferable return; a group method (`Math.foo`)
    /// only promises that the operation is supported, not its shape.
    pub(crate) fn s3_specific_or_group_return(&self, specific: bool, slot: usize) -> RType {
        if specific {
            self.read_return_slot(slot)
        } else {
            RType::unknown()
        }
    }

    /// The call path's post-walk miss tail; operators never reach it,
    /// because a primitive operator is its own fallback in R (issue
    /// #165): a miss there is silent and modeled by the caller's
    /// storage-mode rules. Here, a `<generic>.default` method is a real
    /// dispatch target, so a miss with one stays silent. Otherwise we
    /// report RY050 only for generics with a project-defined method:
    /// an un-stubbed dependency may own a foreign class, while a local
    /// method proves this project owns the dispatch surface.
    fn s3_dispatch_miss(
        &mut self,
        generic: &str,
        generics: &[&str],
        cv: &ClassVector,
        span: Span,
    ) -> Option<RType> {
        let has_default = generics.iter().any(|candidate| {
            let default_key = ((*candidate).to_string(), "default".to_string());
            self.fn_table.s3_methods.contains_key(&default_key)
                || self.typeshed.s3_methods.contains_key(&default_key)
                || self.external_s3_methods.contains(&default_key)
                || self.available_package_names().into_iter().any(|pkg| {
                    self.package_typeshed(pkg)
                        .is_some_and(|typeshed| typeshed.s3_methods.contains_key(&default_key))
                })
        });
        if has_default {
            return Some(RType::unknown());
        }
        let has_known_s3_method =
            self.fn_table.s3_methods.keys().any(|(known_generic, _)| {
                generics.iter().any(|candidate| known_generic == candidate)
            });
        if !has_known_s3_method {
            return None;
        }
        // Math/Summary members have built-in fallbacks. An unrelated local
        // group method cannot make a class-specific method mandatory. Keep
        // the result opaque: the local inventory cannot rule out registered
        // methods, and their return types need not match the primitive.
        if s3_group_generic(generic).is_some() {
            return Some(RType::unknown());
        }
        // The generic has no dispatch target for this class. Emit RY050
        // and return opaque so callers don't trip further diagnostics on
        // the result.
        let classes = cv
            .names
            .iter()
            .take(cv.len as usize)
            .flatten()
            .map(|class| class.as_ref())
            .collect::<Vec<_>>()
            .join(", ");
        self.emit(
            Severity::Warning,
            span,
            "RY050",
            format!(
                "S3 generic `{}` called on value with classes [{}] but no matching method is defined",
                generic, classes,
            ),
        );
        Some(RType::unknown())
    }
}

/// One method-source hit in the S3 dispatch ladder shared by call and
/// operator dispatch.
pub(crate) enum S3MethodSource {
    /// Registered through package metadata: not analyzable, so dispatch
    /// can only conclude opaque.
    Registered,
    /// A project-defined method (`generic.class <- function(...)`) and
    /// its refined return slot.
    Project(usize),
    /// A stub signature from the base typeshed or a package typeshed.
    /// Boxed: `FunctionSig` is large, and the ladder usually misses.
    Stub(Box<FunctionSig>),
}

/// S3 group generics used by ordinary function calls. Operator expressions
/// are handled in `infer/binop.rs`; these names cover calls such as
/// `abs(x)` and `sum(x)` dispatching to `Math.foo` / `Summary.foo`.
///
/// The member sets live in the semantic registry
/// ([`crate::semantic_lists::S3_MATH_GENERICS`] and
/// [`crate::semantic_lists::S3_SUMMARY_GENERICS`]), where the coherence
/// tests pin each member to the embedded base typeshed.
pub(crate) fn s3_group_generic(generic: &str) -> Option<&'static str> {
    if crate::semantic_lists::S3_MATH_GENERICS.contains(&generic) {
        Some("Math")
    } else if crate::semantic_lists::S3_SUMMARY_GENERICS.contains(&generic) {
        Some("Summary")
    } else {
        None
    }
}

fn higher_order_mode(mode: Option<&str>) -> Mode {
    match mode.and_then(JsonMode::parse) {
        Some(JsonMode::Logical) => Mode::Logical,
        Some(JsonMode::Integer) => Mode::Integer,
        Some(JsonMode::Double) => Mode::Double,
        Some(JsonMode::Character) => Mode::Character,
        _ => Mode::Opaque,
    }
}
