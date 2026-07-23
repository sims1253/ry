use super::*;

impl Checker {
    pub(crate) fn infer_binop(
        &mut self,
        op: BinOpKind,
        lt: RType,
        rt: RType,
        span: Span,
        known_null_is_actionable: bool,
    ) -> RType {
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
        // `Ops.data.frame` is implemented by base R, but its stub is
        // necessarily opaque. Keep the useful record shape here instead of
        // letting that opaque S3 result erase it.
        if let Some(result) = data_frame_binop_result(op, &lt, &rt) {
            return result;
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
                self.emit_scalar_logical_length(op, lt.length, span, false);
                self.emit_scalar_logical_length(op, rt.length, span, false);
            }
            return RType::new(Mode::Logical, length);
        }
        // Arithmetic.
        let lt_mode = lt.mode;
        let rt_mode = rt.mode;
        // Arithmetic with a known NULL is never a useful numeric operation:
        // base R returns a zero-length numeric vector for numeric operands
        // (and errors for some other modes).  Do this before the lattice
        // operation, which deliberately models that runtime result.  A
        // union that merely contains NULL remains speculative and is left to
        // the normal lattice path.
        if known_null_is_actionable
            && (matches!(lt_mode, Mode::Null) || matches!(rt_mode, Mode::Null))
        {
            self.emit(
                Severity::Error,
                span,
                "RY040",
                "arithmetic with `NULL` produces `numeric(0)`; the operand is known to be NULL",
            );
            return RType::unknown();
        }
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
            for class in operand
                .class
                .names
                .iter()
                .take(operand.class.len as usize)
                .flatten()
            {
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
                        // A specific operator method has an inferable return;
                        // a group method only promises that this operator is
                        // supported, not its result shape.
                        return Some(if generic == symbol {
                            self.return_slots.get(*slot)
                        } else {
                            RType::unknown()
                        });
                    }
                }
            }
        }
        None
    }

    pub(crate) fn try_s3_unary_dispatch(&self, op: UnaryOpKind, operand: &RType) -> Option<RType> {
        if operand.class.is_unknown() && !matches!(operand.mode, Mode::Opaque | Mode::Union) {
            return Some(RType::unknown());
        }
        let symbol = match op {
            UnaryOpKind::Neg => "-",
            UnaryOpKind::Not => "!",
        };
        for class in operand
            .class
            .names
            .iter()
            .take(operand.class.len as usize)
            .flatten()
        {
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
                    return Some(if generic == symbol {
                        self.return_slots.get(*slot)
                    } else {
                        RType::unknown()
                    });
                }
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
        let rhs_parameter_vector = self.short_circuit_parameter_vector(op, lhs, rhs, scope);
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
        let before = self.diagnostics.len();
        let result = self.infer_binop(
            op,
            lt,
            rt,
            span,
            known_null_arithmetic_operand(lhs, scope) || known_null_arithmetic_operand(rhs, scope),
        );
        if rhs_parameter_vector
            && !self.diagnostics[before..]
                .iter()
                .any(|diagnostic| diagnostic.code == "RY032")
        {
            self.emit(
                Severity::Warning,
                span,
                "RY032",
                format!(
                    "`{}` operand depends on a parameter whose length is not known to be 1; current R errors for vectors",
                    op_symbol(op)
                ),
            );
        }
        result
    }

    fn emit_scalar_logical_length(
        &mut self,
        op: BinOpKind,
        length: Length,
        span: Span,
        unknown_is_actionable: bool,
    ) {
        match length {
            Length::Known(n) if n > 1 => self.emit(
                Severity::Warning,
                span,
                "RY032",
                format!(
                    "`{}` applied to a length-{} operand; only the first element is used",
                    op_symbol(op),
                    n
                ),
            ),
            Length::Unknown if unknown_is_actionable => self.emit(
                Severity::Warning,
                span,
                "RY032",
                format!(
                    "`{}` operand length is not known to be 1; current R errors for vectors",
                    op_symbol(op)
                ),
            ),
            _ => {}
        }
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

/// Recognize the high-confidence parameter guard patterns found in package
/// code. Unknown length by itself is not actionable: scalar parameters are
/// common, and widening every `&&`/`||` would violate ry's silence-first bar.
/// These forms, however, explicitly test a possibly empty parameter and then
/// feed the un-scalarized value into a vectorized predicate.
impl Checker {
    fn short_circuit_parameter_vector(
        &self,
        op: BinOpKind,
        lhs: &Expr,
        rhs: &Expr,
        scope: &Scope,
    ) -> bool {
        fn direct_parameter<'a>(expr: &'a Expr, scope: &Scope) -> Option<&'a str> {
            match expr {
                Expr::Ident { name, .. } if scope.is_parameter(name) => Some(name),
                _ => None,
            }
        }

        fn call_on_parameter<'a>(expr: &'a Expr, names: &[&str], scope: &Scope) -> Option<&'a str> {
            let Expr::Call { func, args, .. } = expr else {
                return None;
            };
            let name = ident_name(func)?;
            let bare = name.rsplit_once("::").map(|(_, bare)| bare).unwrap_or(name);
            if !names.contains(&bare) {
                return None;
            }
            direct_parameter(&args.first()?.value, scope)
        }

        fn length_guard_parameter<'a>(expr: &'a Expr, scope: &Scope) -> Option<&'a str> {
            if let Some(parameter) = call_on_parameter(expr, &["length"], scope) {
                return Some(parameter);
            }
            let Expr::BinOp { lhs, rhs, .. } = expr else {
                return None;
            };
            call_on_parameter(lhs, &["length"], scope)
                .or_else(|| call_on_parameter(rhs, &["length"], scope))
        }

        fn vector_predicate_parameter<'a>(expr: &'a Expr, scope: &Scope) -> Option<&'a str> {
            match expr {
                Expr::BinOp {
                    op:
                        BinOpKind::Lt
                        | BinOpKind::Le
                        | BinOpKind::Gt
                        | BinOpKind::Ge
                        | BinOpKind::Eq
                        | BinOpKind::Ne
                        | BinOpKind::In,
                    lhs,
                    rhs,
                    ..
                } => direct_parameter(lhs, scope).or_else(|| direct_parameter(rhs, scope)),
                Expr::UnaryOp {
                    op: UnaryOpKind::Not,
                    expr,
                    ..
                } => vector_predicate_parameter(expr, scope),
                Expr::Call { .. } => call_on_parameter(expr, &["is.na", "grepl", "nzchar"], scope),
                _ => None,
            }
        }

        let guarded = match op {
            BinOpKind::OrOr => call_on_parameter(lhs, &["is.null"], scope),
            BinOpKind::AndAnd => length_guard_parameter(lhs, scope),
            _ => None,
        };
        let Some(parameter) =
            guarded.filter(|parameter| vector_predicate_parameter(rhs, scope) == Some(*parameter))
        else {
            return false;
        };
        self.vector_intent_parameters
            .last()
            .is_some_and(|parameters| parameters.contains(parameter))
    }
}

/// Model the base `Ops.data.frame` method without losing the table's schema.
/// Comparisons produce a logical matrix-like object, for which opaque is the
/// least misleading v1 representation. Arithmetic keeps the frame shape for
/// a scalar counterpart; otherwise it retains column names but not column
/// element types.
fn data_frame_binop_result(op: BinOpKind, lhs: &RType, rhs: &RType) -> Option<RType> {
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
    if !(is_compare
        || is_logic
        || matches!(
            op,
            BinOpKind::Add
                | BinOpKind::Sub
                | BinOpKind::Mul
                | BinOpKind::Div
                | BinOpKind::Pow
                | BinOpKind::Mod
                | BinOpKind::IDiv
        ))
    {
        return None;
    }
    let (frame, other) = if lhs.class.contains("data.frame") {
        (lhs, rhs)
    } else if rhs.class.contains("data.frame") {
        (rhs, lhs)
    } else {
        return None;
    };
    if is_compare || is_logic {
        return Some(RType::unknown());
    }
    let mut result = RType::new(Mode::List, frame.length).with_class(frame.class.clone());
    if let Some(schema) = &frame.columns {
        let keep_types = !other.class.contains("data.frame") && matches!(other.length, Length::One);
        result = result.with_columns(Arc::new(ColumnSchema {
            columns: schema
                .columns
                .iter()
                .map(|(name, ty)| {
                    (
                        name.clone(),
                        if keep_types {
                            ty.clone()
                        } else {
                            RType::unknown()
                        },
                    )
                })
                .collect(),
            complete: schema.complete,
            locally_constructed: schema.locally_constructed,
        }));
    }
    Some(result)
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
