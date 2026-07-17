use super::*;
use crate::infer::*;

impl Checker {
    pub(crate) fn infer_higher_order_call(
        &mut self,
        name: &str,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
        span: Span,
    ) -> Option<RType> {
        let spec = self.resolve_typeshed_sig(name)?.higher_order?;
        Some(self.infer_ho_result(name, &spec, args, arg_types, scope, span))
    }

    /// Per-builtin result-type computation. Used by both pass 2 (pure,
    /// via `infer_discarding`) and pass 3 (diagnostic-emitting). This is
    /// the pass-3 entry point: it calls `self.infer` on data
    /// arguments (which may emit RY010 etc.) before computing the
    /// element type.
    pub(crate) fn infer_ho_result(
        &mut self,
        name: &str,
        spec: &HigherOrderSpec,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
        span: Span,
    ) -> RType {
        let callback_types = self.higher_order_callback_types(spec, arg_types);
        let callback = Self::extract_callback(
            args,
            &[spec.callback_param.as_str()],
            spec.callback_position,
        );
        let callback_return = callback
            .and_then(|callback| self.callback_return_type(callback, &callback_types, scope));
        match spec.result.kind {
            HigherOrderResultKind::ListOfCallbackReturn => {
                let length = spec
                    .result
                    .length_arg
                    .and_then(|i| arg_types.get(i))
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
                if spec.result.length_arg.is_some()
                    && let Some(return_type) = &callback_return
                    && !modes_compatible(&return_type.mode, &mode)
                {
                    let bare_name = name.rsplit_once("::").map(|(_, bare)| bare).unwrap_or(name);
                    self.emit(
                        Severity::Warning,
                        span,
                        "RY080",
                        format!(
                            "`{bare_name}` expects `{mode}` returns but the callback returns `{}`; R will coerce silently",
                            return_type.mode
                        ),
                    );
                }
                let length = spec
                    .result
                    .length_arg
                    .and_then(|i| arg_types.get(i))
                    .map(|ty| ty.length)
                    .unwrap_or(Length::One);
                RType::new(mode, length)
            }
            HigherOrderResultKind::SameAsArg0 => {
                let mut result = arg_types
                    .get(spec.result.source_arg.unwrap_or(0))
                    .cloned()
                    .unwrap_or_else(RType::unknown);
                if spec.result.unknown_length {
                    result.length = Length::Unknown;
                }
                result
            }
            HigherOrderResultKind::CallbackReturn => {
                if let Some(index) = spec.result.source_arg {
                    arg_types
                        .get(index)
                        .map(RType::element)
                        .unwrap_or_else(RType::unknown)
                } else {
                    callback_return.unwrap_or_else(RType::unknown)
                }
            }
            HigherOrderResultKind::FirstArg => arg_types
                .get(spec.result.source_arg.unwrap_or(0))
                .cloned()
                .unwrap_or_else(RType::unknown),
            HigherOrderResultKind::Simplify => {
                if spec.callback_args == [CallbackArg::Unknown] {
                    return self.ho_rapply(args, arg_types, scope);
                }
                match callback_return {
                    Some(ty)
                        if matches!(ty.length, Length::One)
                            && !matches!(ty.mode, Mode::List | Mode::Opaque | Mode::Union) =>
                    {
                        let length = spec
                            .result
                            .length_arg
                            .and_then(|i| arg_types.get(i))
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
                    .and_then(|i| arg_types.get(i))
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
                        .and_then(|i| arg_types.get(i))
                        .map(|ty| ty.length)
                        .unwrap_or(Length::Unknown)
                } else {
                    Length::Unknown
                };
                RType::new(mode, length)
            }
            HigherOrderResultKind::CallbackIdentity => self.ho_callback_identity(spec, args, scope),
        }
    }

    fn higher_order_callback_types(
        &self,
        spec: &HigherOrderSpec,
        arg_types: &[RType],
    ) -> Vec<RType> {
        let mut types = Vec::new();
        for callback_arg in &spec.callback_args {
            match callback_arg {
                CallbackArg::ElementOfArg0 => types.push(
                    arg_types
                        .first()
                        .map(RType::element)
                        .unwrap_or_else(RType::unknown),
                ),
                CallbackArg::ElementOfArg1 => types.push(
                    arg_types
                        .get(1)
                        .map(RType::element)
                        .unwrap_or_else(RType::unknown),
                ),
                CallbackArg::Unknown => types.push(RType::unknown()),
                CallbackArg::AccumulatorAndElement => {
                    let data_index = if spec.callback_position == 0 { 1 } else { 0 };
                    let element = arg_types
                        .get(data_index)
                        .map(RType::element)
                        .unwrap_or_else(RType::unknown);
                    types.extend([element.clone(), element]);
                }
                CallbackArg::ElementsAfterCallback => types.extend(
                    arg_types
                        .iter()
                        .skip(spec.callback_position + 1)
                        .map(RType::element),
                ),
            }
        }
        types
    }

    /// Extract the callback expression from an argument list by name
    /// (`FUN`, `f`) or by positional index. Returns `None` when no
    /// callback argument is present.
    pub(crate) fn extract_callback<'a>(
        args: &'a [Arg],
        names: &[&str],
        positional_idx: usize,
    ) -> Option<&'a Expr> {
        for a in args {
            if let Some(n) = a.name.as_deref() {
                if names.contains(&n) {
                    return Some(&a.value);
                }
            }
        }
        args.get(positional_idx).map(|a| &a.value)
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
        scope: &Scope,
    ) -> RType {
        let cb = match Self::extract_callback(
            args,
            &[spec.callback_param.as_str()],
            spec.callback_position,
        ) {
            Some(c) => c,
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

    /// `rapply(L, f, ...)`: recursively applies `f` to each leaf of
    /// list `L`. The result is a list of the same shape. We model only
    /// the top-level shape: result is a list with L's length.
    pub(crate) fn ho_rapply(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let l_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        let callback_return = Self::extract_callback(args, &["f", "FUN"], 1)
            .and_then(|cb| self.callback_return_type(cb, &[RType::unknown()], scope));
        let how = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("how"))
            .map(|arg| &arg.value);
        let unlists =
            how.is_none() || matches!(how, Some(Expr::String(value, _)) if value == "unlist");
        if unlists {
            if let Some(ret) = callback_return {
                if matches!(
                    ret.mode,
                    Mode::Logical
                        | Mode::Integer
                        | Mode::Double
                        | Mode::Complex
                        | Mode::Character
                        | Mode::Raw
                ) {
                    return RType::new(ret.mode, Length::Unknown);
                }
            }
        }
        RType::new(Mode::List, l_type.length)
    }

    /// Infer the return type of a single callback invocation, given the
    /// argument types the higher-order function will pass to it.
    ///
    /// Covers three callback forms:
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
                // `rsplit_once("::")` handles both `::` and `:::`.
                let lookup_name = name
                    .rsplit_once("::")
                    .map(|(_, n)| n)
                    .unwrap_or(name.as_str());
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
                        let mut returns = members.iter().map(|member| {
                            member
                                .fn_sig
                                .as_ref()
                                .map(|signature| (*signature.return_type).clone())
                                .unwrap_or_else(RType::unknown)
                        });
                        let first = returns.next().unwrap_or_else(RType::unknown);
                        return Some(returns.fold(first, RType::join));
                    }
                }
                // User-defined function in the FnTable?
                if let Some(f) = self.fn_table.fns.get(lookup_name) {
                    let rt = self.return_slots.get(f.return_slot);
                    if !matches!(rt.mode, Mode::Opaque) {
                        return Some(rt);
                    }
                    return None;
                }
                // Typeshed function?
                if let Some(sig) = self.resolve_typeshed_sig(name) {
                    return Some(self.apply_sig(
                        lookup_name,
                        &sig,
                        call_arg_types,
                        &[],
                        Span::default(),
                    ));
                }
                None
            }
            _ => None,
        }
    }

    /// Walk an anonymous function literal's body to infer its return
    /// type, given the argument types the caller will pass. Similar to
    /// `build_function_signature` but takes explicit argument
    /// types rather than inferring from defaults. Used by
    /// `callback_return_type` for the inline-literal case.
    pub(crate) fn callback_literal_return(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        call_arg_types: &[RType],
        captured_scope: &Scope,
        depth: usize,
    ) -> Option<RType> {
        if body.is_empty() || depth >= MAX_CLOSURE_DEPTH {
            return None;
        }
        // Pure return-type computation: force discarding so this does not
        // double-emit diagnostics (the callback body's diagnostics come
        // from walk_callback_for_diagnostics in pass 3).
        let prev_discarding = self.discarding;
        self.discarding = true;
        let mut scope = captured_scope.clone();
        for (i, p) in params.iter().enumerate() {
            let t = call_arg_types.get(i).cloned().unwrap_or(RType::unknown());
            scope.insert(p.name.clone(), t);
        }
        let mut returns: Vec<RType> = Vec::new();
        for s in body {
            self.walk_stmt(s, &mut scope, Some(&mut returns));
        }
        if let Some(t) = self.trailing_return_type(body, &mut scope, depth + 1) {
            returns.push(t);
        }
        self.discarding = prev_discarding;
        if returns.is_empty() {
            return None;
        }
        let mut iter = returns.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
        let joined = iter.fold(first, |acc, t| acc.join(t));
        if matches!(joined.mode, Mode::Opaque) {
            return None;
        }
        Some(joined)
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
    pub(crate) fn walk_callback_for_diagnostics(
        &mut self,
        name: &str,
        args: &[Arg],
        arg_types: &[RType],
        scope: &mut Scope,
    ) {
        let spec = match self
            .resolve_typeshed_sig(name)
            .and_then(|signature| signature.higher_order)
        {
            Some(spec) => spec,
            None => return,
        };
        if matches!(spec.result.kind, HigherOrderResultKind::CallbackIdentity) {
            return;
        }
        let elem_types = self.higher_order_callback_types(&spec, arg_types);
        let cb = match Self::extract_callback(
            args,
            &[spec.callback_param.as_str()],
            spec.callback_position,
        ) {
            Some(c) => c,
            None => return,
        };
        // Look through a `purrr::in_parallel(.f)` wrapper so the inner
        // function's body is walked (in_parallel is type-transparent).
        let cb = self.unwrap_callback_identity(cb);
        if let Expr::Function { params, body, .. } = cb {
            let mut fn_scope = scope.clone();
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
    /// caller should fall through to other resolution paths.
    ///
    /// RY050 emission policy: a `<generic>.default` method is a real S3
    /// dispatch target, not merely evidence that `generic` uses S3. When
    /// it exists in any method source, a miss for a class-specific method
    /// falls through to it and must remain silent. Without a default, we
    /// report only for a generic that has at least one project-defined S3
    /// method. This is the conservative cross-package gate: an un-stubbed
    /// dependency may own a foreign class, while a local method proves that
    /// this project owns the generic's dispatch surface.
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
            .chain(s3_group_generic(generic).into_iter())
            .collect::<Vec<_>>();
        // R tries each class in order. For every class, the specific generic
        // wins over its group generic (e.g. `abs.foo` before `Math.foo`).
        for class in cv.names.iter().take(cv.len as usize).flatten() {
            if &**class == "default" {
                continue;
            }
            for candidate in &generics {
                let key = ((*candidate).to_string(), class.to_string());
                if self.external_s3_methods.contains(&key) {
                    return Some(RType::unknown());
                }
                if let Some(slot) = self.fn_table.s3_methods.get(&key).cloned() {
                    return Some(if *candidate == generic {
                        self.return_slots.get(slot)
                    } else {
                        RType::unknown()
                    });
                }
                if let Some(sig) = self.typeshed.s3_methods.get(&key).cloned() {
                    return Some(self.apply_sig(candidate, &sig, arg_types, &[], span));
                }
                for pkg in self.available_package_names() {
                    if let Some(sig) = self
                        .package_typeshed(pkg)
                        .and_then(|t| t.s3_methods.get(&key))
                        .cloned()
                    {
                        return Some(self.apply_sig(candidate, &sig, arg_types, &[], span));
                    }
                }
            }
        }
        // 3. A default method is the final S3 dispatch fallback. Consult
        // every source used for specific methods above (plus external
        // registrations), and do not report a missing class method when
        // dispatch can reach one.
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

        // 4. Without a default, use the project-owned-method fallback gate.
        // External/typeshed methods alone cannot prove that this project owns
        // the class, so suppress RY050 for potentially un-stubbed packages.
        let has_known_s3_method =
            self.fn_table.s3_methods.keys().any(|(known_generic, _)| {
                generics.iter().any(|candidate| known_generic == candidate)
            });
        if !has_known_s3_method {
            return None;
        }
        // The generic has no dispatch target for this class. Emit RY050
        // and return opaque so callers don't trip further diagnostics on
        // the result.
        self.emit(
            Severity::Warning,
            span,
            "RY050",
            format!(
                "S3 generic `{}` called on value with classes [{}] but no matching method is defined",
                generic,
                cv.names.iter().take(cv.len as usize).flatten().map(|class| class.as_ref()).collect::<Vec<_>>().join(", "),
            ),
        );
        Some(RType::unknown())
    }
}

/// S3 group generics used by ordinary function calls. Operator expressions
/// are handled in `infer/binop.rs`; these names cover calls such as
/// `abs(x)` and `sum(x)` dispatching to `Math.foo` / `Summary.foo`.
pub(crate) fn s3_group_generic(generic: &str) -> Option<&'static str> {
    match generic {
        "abs" | "acos" | "acosh" | "asin" | "asinh" | "atan" | "atanh" | "ceiling" | "cos"
        | "cosh" | "exp" | "expm1" | "floor" | "gamma" | "lgamma" | "log" | "log10" | "log1p"
        | "log2" | "round" | "sign" | "sin" | "sinh" | "sqrt" | "tan" | "tanh" | "trunc" => {
            Some("Math")
        }
        "all" | "any" | "max" | "min" | "prod" | "range" | "sum" => Some("Summary"),
        _ => None,
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
