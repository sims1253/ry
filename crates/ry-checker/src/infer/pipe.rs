use super::*;

/// Which pipe operator introduced the RHS. The two forms use different
/// placeholders: magrittr binds `.`, base R's native pipe binds `_`
/// (R 4.2+). A placeholder only resolves to the LHS in its own form;
/// in the other form it is an ordinary identifier reference.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipeForm {
    /// `%>%`, `%T>%`, `%<>%`.
    Magrittr,
    /// `|>`.
    Native,
}

impl PipeForm {
    pub(crate) fn of(op: BinOpKind) -> Self {
        match op {
            BinOpKind::PipeNative => PipeForm::Native,
            _ => PipeForm::Magrittr,
        }
    }

    /// True if `e` is the placeholder belonging to this pipe form.
    fn is_placeholder(self, e: &Expr) -> bool {
        match self {
            PipeForm::Magrittr => matches!(e, Expr::Ident { name, .. } if name == "."),
            PipeForm::Native => matches!(e, Expr::Ident { name, .. } if name == "_"),
        }
    }
}

/// True if `e` is an extraction chain rooted at `form`'s placeholder,
/// e.g. `_$mpg`, `_[["mpg"]][2]` or `.$mpg[1]`.
fn is_placeholder_chain(e: &Expr, form: PipeForm) -> bool {
    match e {
        Expr::Index { base, .. } => form.is_placeholder(base) || is_placeholder_chain(base, form),
        _ => false,
    }
}

impl Checker {
    /// Run `f` with magrittr's `.` bound to the piped value, restoring the
    /// previous binding (if any) afterwards. magrittr binds `.` across the
    /// whole RHS, so pronouns nested inside call arguments or subscripts
    /// (`df %>% .[.$mpg > 20, ]`) resolve to the piped value too. The
    /// native `_` is deliberately not bound: R accepts it only in specific
    /// positions, which are matched structurally instead.
    fn with_dot_bound<R>(
        &mut self,
        form: PipeForm,
        lhs_t: &RType,
        scope: &mut Scope,
        f: impl FnOnce(&mut Self, &mut Scope) -> R,
    ) -> R {
        let restore = matches!(form, PipeForm::Magrittr)
            .then(|| scope.bindings.insert(".".to_string(), lhs_t.clone()));
        let result = f(self, scope);
        if let Some(previous) = restore {
            match previous {
                Some(t) => scope.bindings.insert(".".to_string(), t),
                None => scope.bindings.remove("."),
            };
        }
        result
    }

    /// Infer an extraction chain rooted at a pipe placeholder by applying
    /// each index operation to `lhs_t` from the root outwards. Returns
    /// `None` if `e` is not such a chain.
    fn infer_placeholder_chain(
        &mut self,
        e: &Expr,
        lhs_t: RType,
        form: PipeForm,
        span: Span,
        scope: &mut Scope,
    ) -> Option<RType> {
        let Expr::Index {
            base, kind, args, ..
        } = e
        else {
            return None;
        };
        let base_t = if form.is_placeholder(base) {
            lhs_t
        } else {
            self.infer_placeholder_chain(base, lhs_t, form, span_of(base), scope)?
        };
        Some(self.infer_index(base_t, *kind, args, span, false, scope))
    }

    pub(crate) fn infer_pipe(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        form: PipeForm,
        scope: &mut Scope,
    ) -> RType {
        // Infer the LHS so diagnostics fire on it (e.g. unbound name).
        let lhs_t = self.infer(lhs, scope);
        self.infer_pipe_with_lhs_type(lhs, rhs, span, lhs_t, form, scope)
    }

    /// Infer a pipe RHS after its LHS has already been inferred in `scope`.
    ///
    /// The pipe desugaring injects a clone of `lhs` as a call argument. Keep
    /// its type available only for the duration of that call so `infer` can
    /// reuse it instead of recursively re-inferencing the whole pipe chain.
    fn infer_pipe_with_lhs_type(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        lhs_t: RType,
        form: PipeForm,
        scope: &mut Scope,
    ) -> RType {
        match rhs {
            // Pipe placeholder with nested access: magrittr's
            // `df %>% .$col`, `df %>% .[i]`, `df %>% .[[i]]` and base R's
            // extraction placeholder (R >= 4.3) `df |> _$col`,
            // `df |> _[["col"]]`. The placeholder at the root of the
            // extraction resolves to the piped LHS value, so we infer the
            // chain against `lhs_t` directly. Only the placeholder
            // belonging to this pipe form counts: `df |> .$col` reads an
            // ordinary (and here unbound) `.`, so it falls through to the
            // generic arm below and keeps its normal index inference.
            Expr::Index { .. } if is_placeholder_chain(rhs, form) => {
                let chain_t = lhs_t.clone();
                self.with_dot_bound(form, &lhs_t, scope, |checker, scope| {
                    checker
                        .infer_placeholder_chain(rhs, chain_t, form, span, scope)
                        .unwrap_or_else(RType::unknown)
                })
            }
            // A braced magrittr RHS is a unary lambda whose `.` pronoun is
            // bound to the LHS (`x %>% { .$field == value }`).
            Expr::Block { body, .. } => {
                let mut inner = scope.clone();
                inner.insert(".", lhs_t);
                let Some((last, prefix)) = body.split_last() else {
                    return RType::new(Mode::Null, Length::Zero);
                };
                for statement in prefix {
                    self.walk_stmt(statement, &mut inner, None);
                }
                self.infer_stmt_value(last, &mut inner)
            }
            // Bare magrittr pronoun: `x %>% .` returns the LHS value
            // itself (the `.` refers to the LHS). This is distinct from
            // the general `Ident` arm below, which would treat `.` as a
            // function name and call `.(lhs)`.
            Expr::Ident { .. } if form.is_placeholder(rhs) => lhs_t,
            Expr::Call {
                func,
                args,
                span: call_span,
            } => {
                let mut new_args: Vec<Arg> = Vec::with_capacity(args.len() + 1);
                let mut placeholder_seen = false;
                for a in args {
                    // magrittr substitutes *every* `.` argument, so
                    // `x %>% paste(., ., sep = "-")` pipes `x` into both.
                    // The native pipe permits a single `_`, so only its
                    // first occurrence is substituted.
                    let substitute = form.is_placeholder(&a.value)
                        && (!placeholder_seen || matches!(form, PipeForm::Magrittr));
                    if substitute {
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
                // Nested pronouns resolve through the binding rather than
                // the substitution above: `x %>% sum(rev(.))` is
                // `sum(x, rev(x))`. Only a *top-level* `.` suppresses the
                // prepended LHS, which is why both mechanisms are needed.
                let call_t = lhs_t.clone();
                self.with_dot_bound(form, &lhs_t, scope, |checker, scope| {
                    checker.infer_pipe_call(func, &new_args, lhs, call_t, scope, *call_span)
                })
            }
            Expr::Ident { .. } => {
                let new_args = vec![Arg {
                    name: None,
                    value: lhs.clone(),
                    span,
                }];
                self.infer_pipe_call(rhs, &new_args, lhs, lhs_t.clone(), scope, span)
            }
            _ => {
                // Unknown rhs form: infer rhs for diagnostics, give up on type.
                let _ = self.infer(rhs, scope);
                RType::unknown()
            }
        }
    }

    fn infer_pipe_call(
        &mut self,
        func: &Expr,
        args: &[Arg],
        lhs: &Expr,
        lhs_t: RType,
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        let lhs_span = span_of(lhs);
        let previous = self.pipe_argument_types.insert(lhs_span, lhs_t);
        let result = self.infer_call(func, args, scope, span);
        if let Some(previous) = previous {
            self.pipe_argument_types.insert(lhs_span, previous);
        } else {
            self.pipe_argument_types.remove(&lhs_span);
        }
        result
    }

    /// Tee pipe `%T>%`: run both sides for diagnostics, return the LHS type.
    /// The RHS side-effect (e.g. `print`, `plot`) is discarded at runtime;
    /// the value flows through as the LHS.
    pub(crate) fn infer_pipe_tee(&mut self, lhs: &Expr, rhs: &Expr, scope: &mut Scope) -> RType {
        let lhs_t = self.infer(lhs, scope);
        // Still walk the RHS so any diagnostics on its body fire.
        let _ = self.infer_pipe_with_lhs_type(
            lhs,
            rhs,
            span_of(rhs),
            lhs_t.clone(),
            PipeForm::Magrittr,
            scope,
        );
        lhs_t
    }

    /// Infer the type of an `if` expression `if (cond) then else else_`.
    /// The condition is inferred for diagnostics (RY001/RY002/RY003). Both
    /// branches are inferred; the result is the join of their types.
    /// When `else_` is absent, R returns NULL for the else branch, so
    /// we join with NULL's type.
    pub(crate) fn infer_if_expr(
        &mut self,
        cond: &Expr,
        then: &Expr,
        else_: &Option<Box<Expr>>,
        _span: Span,
        scope: &mut Scope,
    ) -> RType {
        // RY103: an `if` used in expression position still requires a
        // length-1 logical condition.
        self.check_class_equality_operand(cond, scope);
        let diagnostic_start = self.diagnostics.len();
        let ct = self.infer(cond, scope);
        self.emit_condition_diagnostics(cond, ct, scope, diagnostic_start, ConditionContext::If);
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
        let narrowing = self.extract_type_narrowing(cond, scope);
        let (mut then_scope, mut else_scope, _narrowed) = apply_narrowing(scope, &narrowing);
        let then_t = self.infer(then, &mut then_scope);
        let else_t = match else_ {
            Some(e) => self.infer(e, &mut else_scope),
            None => RType::new(Mode::Null, Length::Zero),
        };
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
        _span: Span,
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
        _span: Span,
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
        if types.is_empty() {
            return RType::unknown();
        }
        let mut iter = types.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
        iter.fold(first, |acc, t| acc.join(t))
    }
}
