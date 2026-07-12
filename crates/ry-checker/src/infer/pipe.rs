use super::*;

impl Checker {
    pub(crate) fn infer_pipe(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        scope: &mut Scope,
    ) -> RType {
        // Infer the LHS so diagnostics fire on it (e.g. unbound name).
        let lhs_t = self.infer(lhs, scope);
        let result = match rhs {
            // Magrittr data pronoun with nested access:
            // `df %>% .$col`, `df %>% .[i]`, `df %>% .[[i]]`. The `.` at
            // the base of the index resolves to the piped LHS value, so
            // we infer the index against `lhs_t` directly.
            Expr::Index {
                base, kind, args, ..
            } if is_dot_pronoun(base) => self.infer_index(lhs_t, *kind, args, span, scope),
            // Bare magrittr pronoun: `x %>% .` returns the LHS value
            // itself (the `.` refers to the LHS). This is distinct from
            // the general `Ident` arm below, which would treat `.` as a
            // function name and call `.(lhs)`.
            Expr::Ident { name, .. } if name == "." => lhs_t,
            Expr::Call {
                func,
                args,
                span: call_span,
            } => {
                let mut new_args: Vec<Arg> = Vec::with_capacity(args.len() + 1);
                let mut placeholder_seen = false;
                for a in args {
                    if !placeholder_seen && is_pipe_placeholder(&a.value) {
                        new_args.push(Arg {
                            name: a.name.clone(),
                            value: lhs.clone(),
                            span: a.span,
                        });
                        placeholder_seen = true;
                    } else {
                        new_args.push(a.clone());
                    }
                }
                if !placeholder_seen {
                    new_args.insert(
                        0,
                        Arg {
                            name: None,
                            value: lhs.clone(),
                            span,
                        },
                    );
                }
                self.infer_call(func, &new_args, scope, *call_span)
            }
            Expr::Ident { .. } => {
                let new_args = vec![Arg {
                    name: None,
                    value: lhs.clone(),
                    span,
                }];
                self.infer_call(rhs, &new_args, scope, span)
            }
            _ => {
                // Unknown rhs form: infer rhs for diagnostics, give up on type.
                let _ = self.infer(rhs, scope);
                RType::unknown()
            }
        };
        let _ = lhs_t;
        result
    }

    /// Tee pipe `%T>%`: run both sides for diagnostics, return the LHS type.
    /// The RHS side-effect (e.g. `print`, `plot`) is discarded at runtime;
    /// the value flows through as the LHS.
    pub(crate) fn infer_pipe_tee(&mut self, lhs: &Expr, rhs: &Expr, scope: &mut Scope) -> RType {
        let lhs_t = self.infer(lhs, scope);
        // Still walk the RHS so any diagnostics on its body fire.
        let _ = self.infer_pipe(lhs, rhs, span_of(rhs), scope);
        lhs_t
    }

    /// Infer the type of an `if` expression `if (cond) then else else_`.
    /// The condition is inferred for diagnostics (RY001/RY002). Both
    /// branches are inferred; the result is the join of their types.
    /// When `else_` is absent, R returns NULL for the else branch, so
    /// we join with NULL's type.
    pub(crate) fn infer_if_expr(
        &mut self,
        cond: &Expr,
        then: &Expr,
        else_: &Option<Box<Expr>>,
        span: Span,
        scope: &mut Scope,
    ) -> RType {
        let ct = self.infer(cond, scope);
        if ct.invalid_condition() {
            self.emit(
                Severity::Error,
                span_of(cond),
                "RY001",
                format!("`if` condition is `{}`, expected length-1 logical", ct),
            );
        } else if !matches!(ct.mode, Mode::Logical | Mode::Opaque)
            && !is_numeric_truthiness_idiom(cond, scope)
        {
            self.emit(
                Severity::Warning,
                span_of(cond),
                "RY001",
                format!(
                    "`if` condition is `{}` (not logical); will be silently coerced",
                    ct.mode
                ),
            );
        } else if matches!(ct.mode, Mode::Logical) {
            if let Length::Known(n) = ct.length {
                if n > 1 {
                    self.emit(
                        Severity::Warning,
                        span_of(cond),
                        "RY002",
                        format!(
                            "`if` condition has length {}; R requires a length-1 condition",
                            n
                        ),
                    );
                }
            }
        }
        // Flow-sensitive type narrowing for the expression form too.
        //
        // Limitation: the branch scopes here are clones, and
        // `BinOpKind::Assign` in expression position (e.g.
        // `y <- if (c) (x <- 1) else (x <- 2); x`) mutates only the clone, so
        // any binding introduced inside an `if` *expression* is silently
        // dropped. The statement-form `Stmt::If` merges its branch bindings
        // back into the parent (see `merge_branch_bindings`); doing the same
        // for the expression form is deferred to a later phase because
        // expression-position assignment is rare and merging here would
        // require plumbing owned branch scopes back to the caller.
        let narrowing = extract_type_narrowing(cond);
        let (then_scope, else_scope, _narrowed) =
            apply_narrowing(scope, &narrowing, else_.is_some());
        let then_t = self.infer(then, &mut then_scope.clone());
        let else_t = match else_ {
            Some(e) => self.infer(e, &mut else_scope.clone()),
            None => RType::new(Mode::Null, Length::Zero),
        };
        let _ = span;
        then_t.join(else_t)
    }

    /// Infer the result type of `switch(EXPR, ...)`. Both forms are
    /// supported:
    ///   * Numeric: `switch(1, "first", "second", "third")` - selects
    ///     the Nth positional argument.
    ///   * Named: `switch(x, a = 1L, b = "two")` - selects by matching
    ///     `x` against the argument names.
    ///
    /// The result type is the join of all alternative types (since we
    /// can't know which branch will execute at runtime). Each
    /// alternative is also walked for diagnostics.
    pub(crate) fn infer_switch_call(
        &mut self,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        // The first argument is the selector; infer it for diagnostics.
        if let Some(first) = args.first() {
            let _ = self.infer(&first.value, scope);
        }
        // Join the types of all remaining arguments (the alternatives).
        let mut alt_types: Vec<RType> = Vec::new();
        for a in args.iter().skip(1) {
            alt_types.push(self.infer(&a.value, scope));
        }
        let _ = span;
        if alt_types.is_empty() {
            return RType::unknown();
        }
        let mut iter = alt_types.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
        iter.fold(first, |acc, t| acc.join(t))
    }

    /// Infer the result type of `tryCatch(expr, ...)`. The first
    /// positional argument is the main expression; subsequent named
    /// arguments are condition handlers (`error = function(e) ...`,
    /// `warning = function(w) ...`, etc.).
    ///
    /// The result type is the join of the main expression's type and
    /// all handler return types. Each handler is a function literal
    /// (or named function); we infer its return type via
    /// `callback_return_type` with the condition object as the
    /// callback's argument (opaque, since we don't model the
    /// condition object).
    pub(crate) fn infer_trycatch_call(
        &mut self,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        let mut types: Vec<RType> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                // Main expression.
                types.push(self.infer(&a.value, scope));
            } else if a.name.is_some() {
                // Named handler: `error = function(e) ...`. Infer the
                // handler function's return type.
                if let Some(rt) = self.callback_return_type(&a.value, &[RType::unknown()], scope) {
                    types.push(rt);
                } else {
                    // Couldn't infer handler return: infer for
                    // diagnostics and use opaque.
                    let _ = self.infer(&a.value, scope);
                }
            } else {
                // Extra positional arg (rare): infer for diagnostics.
                let _ = self.infer(&a.value, scope);
            }
        }
        let _ = span;
        if types.is_empty() {
            return RType::unknown();
        }
        let mut iter = types.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
        iter.fold(first, |acc, t| acc.join(t))
    }
}
