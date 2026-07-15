use super::*;

impl Checker {
    pub(crate) fn infer_binop(&mut self, op: BinOpKind, lt: RType, rt: RType, span: Span) -> RType {
        // `:` sequence operator. Always produces a vector; mode depends
        // on operand modes per R's coercion (int:int -> int, otherwise
        // double). If both operands are integer literals we can even
        // pin the length exactly.
        if matches!(op, BinOpKind::Colon) {
            // Delegate to the type lattice's `seq` method, which models
            // R's `:` behavior (integer for whole-number endpoints).
            return lt.seq(rt);
        }
        // `%in%` matching. In R `x %in% table` returns a logical vector of
        // length(x) -- one membership test per element of the LHS -- and the
        // RHS (`table`) length is irrelevant. Routing it through the generic
        // `compare` path wrongly took `binary(lt.len, rt.len)` (the max), so
        // `x %in% c("a","b")` on a length-1 `x` came out length-2 and drove
        // both RY002 (`if` condition length 2) and RY032 (`&&` on a length-2
        // operand) false positives. `%in%` never errors on mismatched modes
        // (it coerces to a common type), so the result is always plain
        // logical with the LHS length (Unknown LHS length stays Unknown).
        if matches!(op, BinOpKind::In) {
            return RType::new(Mode::Logical, lt.length);
        }
        // Primitive operators dispatch through an operator-specific method
        // (`+.foo`) and then the `Ops.foo` group generic before applying the
        // storage-mode rules below. A dynamically classed value is likewise
        // not proof that the primitive is invalid: its runtime class may
        // provide a method from another package.
        if let Some(dispatched) = self.try_s3_binop_dispatch(op, &lt, &rt) {
            return dispatched;
        }
        let is_compare = matches!(
            op,
            BinOpKind::Lt
                | BinOpKind::Le
                | BinOpKind::Gt
                | BinOpKind::Ge
                | BinOpKind::Eq
                | BinOpKind::Ne
        );
        let is_logic = matches!(
            op,
            BinOpKind::And | BinOpKind::AndAnd | BinOpKind::Or | BinOpKind::OrOr
        );
        if is_compare {
            // Snapshot the operand modes for diagnostics before `compare`
            // consumes lt/rt by value.
            let lt_mode = lt.mode;
            let rt_mode = rt.mode;
            let compares_factor = lt.class.contains("factor") || rt.class.contains("factor");
            // R compares atomic list leaves element-wise for both equality
            // and ordering (`list(1, 2) > 1`). Unknown list element shapes
            // stay opaque; only proven-invalid leaves may produce RY030.
            let comparable_lt = equality_list_leaf_type(op, &lt).unwrap_or_else(|| lt.clone());
            let comparable_rt = equality_list_leaf_type(op, &rt).unwrap_or_else(|| rt.clone());
            if let Some(t) = comparable_lt.compare(comparable_rt) {
                // RY033: warn about comparing a character value with a
                // non-character one. R coerces the numeric operand to
                // character, then compares lexicographically, which is
                // rarely the programmer's intent.
                if matches!(lt_mode, Mode::Character) != matches!(rt_mode, Mode::Character)
                    && !matches!(lt_mode, Mode::Opaque)
                    && !matches!(rt_mode, Mode::Opaque)
                    && !matches!(lt_mode, Mode::Null)
                    && !matches!(rt_mode, Mode::Null)
                    && !matches!(lt_mode, Mode::List | Mode::Function)
                    && !matches!(rt_mode, Mode::List | Mode::Function)
                    && !matches!(lt_mode, Mode::Union)
                    && !matches!(rt_mode, Mode::Union)
                    && !compares_factor
                {
                    self.emit(
                        Severity::Warning,
                        span,
                        "RY033",
                        format!(
                            "comparing `{}` with `{}`; R coerces the numeric value to character and compares lexicographically, which is rarely intended",
                            lt_mode, rt_mode
                        ),
                    );
                }
                if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                    return RType::new(Mode::Logical, Length::One);
                }
                return t;
            }
            self.emit(
                Severity::Error,
                span,
                "RY030",
                format!("cannot compare `{}` with `{}`", lt_mode, rt_mode),
            );
            return RType::unknown();
        }
        if is_logic {
            let lt_mode = lt.mode;
            let rt_mode = rt.mode;
            if matches!(lt_mode, Mode::Character | Mode::List | Mode::Function)
                || matches!(rt_mode, Mode::Character | Mode::List | Mode::Function)
            {
                self.emit(
                    Severity::Error,
                    span,
                    "RY031",
                    format!("logical op applied to `{}` and `{}`", lt_mode, rt_mode),
                );
                return RType::unknown();
            }
            let length = if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                Length::One
            } else {
                lt.length.binary(rt.length)
            };
            if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                if let Length::Known(n) = lt.length {
                    if n > 1 {
                        self.emit(
                            Severity::Warning,
                            span,
                            "RY032",
                            format!("`{}` applied to a length-{} operand; only the first element is used", op_symbol(op), n),
                        );
                    }
                }
                if let Length::Known(n) = rt.length {
                    if n > 1 {
                        self.emit(
                            Severity::Warning,
                            span,
                            "RY032",
                            format!("`{}` applied to a length-{} operand; only the first element is used", op_symbol(op), n),
                        );
                    }
                }
            }
            return RType::new(Mode::Logical, length);
        }
        // Arithmetic.
        let lt_mode = lt.mode;
        let rt_mode = rt.mode;
        let recycles = non_divisible_recycling(lt.length, rt.length);
        let has_factor = lt.class.contains("factor") || rt.class.contains("factor");
        if let Some(t) = lt.arith(rt) {
            if let Some((lhs_len, rhs_len)) = recycles {
                self.emit(
                    Severity::Warning,
                    span,
                    "RY041",
                    format!(
                        "vector lengths {lhs_len} and {rhs_len} do not divide evenly; R will recycle with a warning"
                    ),
                );
            }
            if has_factor {
                self.emit(
                    Severity::Warning,
                    span,
                    "RY042",
                    "arithmetic on a factor produces `NA`; operate on its levels or convert it explicitly",
                );
            }
            return t;
        }
        self.emit(
            Severity::Error,
            span,
            "RY040",
            format!(
                "cannot apply arithmetic op to `{}` and `{}`",
                lt_mode, rt_mode
            ),
        );
        RType::unknown()
    }

    pub(crate) fn try_s3_binop_dispatch(
        &self,
        op: BinOpKind,
        lhs: &RType,
        rhs: &RType,
    ) -> Option<RType> {
        let symbol = op_symbol(op);
        if symbol == "?"
            || matches!(
                op,
                BinOpKind::In | BinOpKind::Colon | BinOpKind::PipeForward
            )
        {
            return None;
        }
        for operand in [lhs, rhs] {
            if operand.class.is_unknown() && !matches!(operand.mode, Mode::Opaque | Mode::Union) {
                return Some(RType::unknown());
            }
            let Some(class) = operand.class.first() else {
                continue;
            };
            for generic in [symbol, "Ops"] {
                if self
                    .external_s3_methods
                    .contains(&(generic.to_string(), class.to_string()))
                {
                    return Some(RType::unknown());
                }
                if let Some(slot) = self
                    .fn_table
                    .s3_methods
                    .get(&(generic.to_string(), class.to_string()))
                {
                    let _ = slot;
                    // Operator methods commonly restore or transform class
                    // attributes through another S3 helper (`c.foo`,
                    // `new_foo`) that the local return inference cannot
                    // represent. Treat the result as opaque rather than
                    // stripping the class and producing a false error on a
                    // chained operator expression.
                    return Some(RType::unknown());
                }
            }
        }
        None
    }

    pub(crate) fn try_s3_unary_dispatch(&self, op: UnaryOpKind, operand: &RType) -> Option<RType> {
        if operand.class.is_unknown() && !matches!(operand.mode, Mode::Opaque | Mode::Union) {
            return Some(RType::unknown());
        }
        let class = operand.class.first()?;
        let symbol = match op {
            UnaryOpKind::Neg => "-",
            UnaryOpKind::Not => "!",
        };
        for generic in [symbol, "Ops"] {
            if self
                .external_s3_methods
                .contains(&(generic.to_string(), class.to_string()))
            {
                return Some(RType::unknown());
            }
            if let Some(slot) = self
                .fn_table
                .s3_methods
                .get(&(generic.to_string(), class.to_string()))
            {
                return Some(self.return_slots.get(*slot));
            }
        }
        None
    }

    pub(crate) fn infer_short_circuit_binop(
        &mut self,
        op: BinOpKind,
        lhs: &Expr,
        rhs: &Expr,
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        let lt = self.infer(lhs, scope);
        let narrowing = extract_type_narrowing(lhs);
        let (then_scope, else_scope, _) = apply_narrowing(scope, &narrowing, true);
        let rt = match op {
            BinOpKind::AndAnd => {
                let mut rhs_scope = then_scope;
                let rt = self.infer(rhs, &mut rhs_scope);
                merge_condition_assignments(scope, &rhs_scope, rhs);
                rt
            }
            BinOpKind::OrOr => {
                let mut rhs_scope = else_scope;
                let rt = self.infer(rhs, &mut rhs_scope);
                merge_condition_assignments(scope, &rhs_scope, rhs);
                rt
            }
            _ => self.infer(rhs, scope),
        };
        self.infer_binop(op, lt, rt, span)
    }

    // Desugar `lhs %>% rhs` (and `lhs |> rhs`, `lhs %<>% rhs`) into a
    // call to `rhs` with `lhs` injected into the argument list.
    //
    // Magrittr `%>%` semantics: if `rhs` is a call, prepend `lhs` as
    // the first positional argument - unless one of the args is the
    // bare placeholder `.` (or base-R `_`), in which case the first
    // such occurrence is replaced with `lhs`. Bare `rhs` (e.g. `x %>% abs`)
    // becomes a one-arg call.
    //
    // Data pronoun: when `rhs` is an index expression whose base is
    // the magrittr `.` pronoun (`df %>% .$col`, `df %>% .[i]`,
    // `df %>% .[[i]]`), the `.` resolves to the piped LHS value and
    // the index is inferred against `lhs`'s type. A bare `x %>% .`
    // returns the LHS value itself.
    //
    // `%<>%` (assignment pipe) shares the result type with `%>%` at v1.
    // The assignment side-effect (`x <- ...`) is handled by the caller
    // when it appears in an `Assign` statement; for a bare binop we
    // cannot reassign without a target expression, so we leave that to
    // a future pass.
}

/// R evaluates assignments nested anywhere in a condition expression in the
/// current function environment. Short-circuit inference uses a cloned scope
/// to model guard narrowing, so copy just those assignment targets back (not
/// the guard refinement itself) after evaluating the RHS.
fn merge_condition_assignments(scope: &mut Scope, evaluated: &Scope, expr: &Expr) {
    let mut names = HashSet::new();
    collect_condition_assignment_names(expr, &mut names);
    for name in names {
        if let Some(ty) = evaluated.get(&name) {
            scope.insert(name, ty.clone());
        }
    }
}

fn collect_condition_assignment_names(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::BinOp { op, lhs, rhs, .. } => {
            if matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign)
                && let Expr::Ident { name, .. } = lhs.as_ref()
            {
                names.insert(name.clone());
            }
            collect_condition_assignment_names(lhs, names);
            collect_condition_assignment_names(rhs, names);
        }
        Expr::Call { func, args, .. } => {
            collect_condition_assignment_names(func, names);
            for arg in args {
                collect_condition_assignment_names(&arg.value, names);
            }
        }
        Expr::UnaryOp { expr, .. } => collect_condition_assignment_names(expr, names),
        Expr::Index { base, args, .. } => {
            collect_condition_assignment_names(base, names);
            for arg in args {
                collect_condition_assignment_names(&arg.value, names);
            }
        }
        Expr::Block { body, .. } => {
            for stmt in body {
                match stmt {
                    Stmt::Assign { target, value, .. } => {
                        if let Expr::Ident { name, .. } = target {
                            names.insert(name.clone());
                        }
                        collect_condition_assignment_names(value, names);
                    }
                    Stmt::Expr(expr) => collect_condition_assignment_names(expr, names),
                    _ => {}
                }
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_condition_assignment_names(cond, names);
            collect_condition_assignment_names(then, names);
            if let Some(else_) = else_ {
                collect_condition_assignment_names(else_, names);
            }
        }
        Expr::Ident { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Function { .. }
        | Expr::Unknown(_) => {}
    }
}
