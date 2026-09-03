use super::*;
use crate::higher_order::S3MethodSource;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr};
use std::ops::ControlFlow;

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
        // storage-mode rules below. Unlike an ordinary generic, a dispatch
        // miss is silent: the primitive itself is the fallback (issue #165's
        // original RY050 criterion was corrected against real R, where
        // defining `+.foo` or `Ops.foo` does not make `bar + 1` warn -- the
        // primitive computes it). The storage-mode rules below are that
        // fallback. A dynamically classed value is likewise not proof that
        // the primitive is invalid: its runtime class may provide a method
        // from another package.
        if let Some(dispatched) = self.try_s3_binop_dispatch(op, &lt, &rt) {
            return dispatched;
        }
        let is_compare = is_comparison(op);
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
            let comparable_lt = equality_list_leaf_type(&lt).unwrap_or_else(|| lt.clone());
            let comparable_rt = equality_list_leaf_type(&rt).unwrap_or_else(|| rt.clone());
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
                self.emit_scalar_logical_length(op, lt.length, span);
                self.emit_scalar_logical_length(op, rt.length, span);
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
        let emit_recycle_warning = |this: &mut Self| {
            if let Some((lhs_len, rhs_len)) = recycles {
                this.emit(
                    Severity::Warning,
                    span,
                    "RY041",
                    format!(
                        "vector lengths {lhs_len} and {rhs_len} do not divide evenly; R will recycle with a warning"
                    ),
                );
            }
        };
        if lt.class.contains("factor") || rt.class.contains("factor") {
            // Base R's `Ops.factor` warns "'+' not meaningful for factors"
            // for *any* arithmetic involving a factor and returns
            // `rep.int(NA, max(length(e1), length(e2)))`, no matter what
            // the other operand is (`factor + 1` and `factor + list`
            // behave alike). Report RY042 before the lattice rules: the
            // dispatched method preempts the primitive's own mode-mismatch
            // error, so a list counterpart must stay a warning, not become
            // RY040. That dispatch equally preempts the primitive's
            // recycling: the method never warns about uneven operand
            // lengths (verified against R 4.6), so no factor path may
            // emit RY041's "R will recycle with a warning" claim -- not
            // even where the storage modes would arith-combine (`factor +
            // 1:2` warns only about meaninglessness in real R).
            self.emit(
                Severity::Warning,
                span,
                "RY042",
                "arithmetic on a factor produces `NA`; operate on its levels or convert it explicitly",
            );
            return lt.arith(rt).unwrap_or_else(RType::unknown);
        }
        if let Some(t) = lt.arith(rt) {
            emit_recycle_warning(self);
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

    /// Resolve operator S3 dispatch for one operand through the shared
    /// method-source ladder (`s3_lookup_method`): the operator's own
    /// method (`+.foo`), then the `Ops` group generic, across project
    /// methods, stub signatures, and external registrations. An unknown
    /// class on a concrete mode means any class vector may apply, so
    /// the result is unknowable. `operands` is the full operator
    /// argument list (both sides of a binary op): a stub signature is
    /// applied to it. `None` is a dispatch miss, which for operators is
    /// silent: unlike an ordinary generic, the primitive itself is R's
    /// fallback, so the caller falls through to the storage-mode rules
    /// below instead of reporting a missing method.
    fn s3_dispatch_on_operand(
        &mut self,
        symbol: &str,
        operands: &[&RType],
        operand: &RType,
    ) -> Option<RType> {
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
            if &**class == "default" {
                continue;
            }
            for generic in [symbol, "Ops"] {
                match self.s3_lookup_method(generic, class) {
                    Some(S3MethodSource::Registered) => return Some(RType::unknown()),
                    Some(S3MethodSource::Project(slot)) => {
                        // A specific operator method has an inferable
                        // return; a group method only promises that this
                        // operator is supported, not its result shape.
                        return Some(self.s3_specific_or_group_return(generic == symbol, slot));
                    }
                    Some(S3MethodSource::Stub(sig)) => {
                        // A specific stub, or a group stub declaring a
                        // usable shape, is this operator's method.
                        let arg_types = operands
                            .iter()
                            .map(|operand| (**operand).clone())
                            .collect::<Vec<_>>();
                        let result = self.apply_sig(&sig, &arg_types, &[]);
                        if generic == symbol || !matches!(result.mode, Mode::Opaque) {
                            return Some(result);
                        }
                        // An opaque group stub (every embedded `Ops.*`
                        // entry) offers no shape: fall through like a
                        // miss, so the storage-mode rules keep modeling
                        // these base classes (`Ops.factor`'s
                        // not-meaningful warning, `Ops.Date` arithmetic)
                        // and keep their diagnostics instead of
                        // collapsing to opaque.
                    }
                    None => {}
                }
            }
        }
        None
    }

    pub(crate) fn try_s3_binop_dispatch(
        &mut self,
        op: BinOpKind,
        lhs: &RType,
        rhs: &RType,
    ) -> Option<RType> {
        // `:`, the pipes, and `%in%` are not S3 generics. `&&`/`||` never
        // dispatch either: in R they are strictly logical short-circuit
        // primitives -- an `Ops.foo` method cannot intercept them -- so
        // their length/type diagnostics below always fire.
        if matches!(
            op,
            BinOpKind::In
                | BinOpKind::Colon
                | BinOpKind::PipeForward
                | BinOpKind::PipeNative
                | BinOpKind::AndAnd
                | BinOpKind::OrOr
        ) {
            return None;
        }
        let symbol = op_symbol(op);
        let operands = [lhs, rhs];
        operands
            .iter()
            .find_map(|operand| self.s3_dispatch_on_operand(symbol, &operands, operand))
    }

    pub(crate) fn try_s3_unary_dispatch(
        &mut self,
        op: UnaryOpKind,
        operand: &RType,
    ) -> Option<RType> {
        let symbol = match op {
            UnaryOpKind::Neg => "-",
            UnaryOpKind::Not => "!",
        };
        self.s3_dispatch_on_operand(symbol, &[operand], operand)
    }

    pub(crate) fn infer_short_circuit_binop(
        &mut self,
        op: BinOpKind,
        lhs: &Expr,
        rhs: &Expr,
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        // RY103: `&&` / `||` coerce each operand to `logical(1)`, so both
        // sides are length-1 logical contexts. Scanning them here (rather
        // than recursing from the enclosing `if`) reports each site exactly
        // once no matter how the operators nest.
        self.check_class_equality_operand(lhs, scope);
        let lt = self.infer(lhs, scope);
        let narrowing = self.extract_type_narrowing(lhs, scope);
        let (then_scope, else_scope, _) = apply_narrowing(scope, &narrowing);
        let rhs_parameter_vector = self.short_circuit_parameter_vector(op, lhs, rhs, scope);
        let rt = match op {
            BinOpKind::AndAnd => {
                self.check_class_equality_operand(rhs, &then_scope);
                let mut rhs_scope = then_scope;
                let rt = self.infer(rhs, &mut rhs_scope);
                merge_condition_assignments(scope, &rhs_scope, rhs);
                rt
            }
            BinOpKind::OrOr => {
                self.check_class_equality_operand(rhs, &else_scope);
                let mut rhs_scope = else_scope;
                let rt = self.infer(rhs, &mut rhs_scope);
                merge_condition_assignments(scope, &rhs_scope, rhs);
                rt
            }
            _ => {
                self.check_class_equality_operand(rhs, scope);
                self.infer(rhs, scope)
            }
        };
        // A parameter guard relies on lazy short-circuit evaluation, so it
        // keeps its distinct warning below rather than the length-based one.
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
            let message = format!(
                "`{}` operand depends on a parameter whose length is not known to be 1; R errors at runtime for vector operands",
                op_symbol(op)
            );
            self.emit(Severity::Warning, span, "RY032", message);
        }
        result
    }

    fn emit_scalar_logical_length(&mut self, op: BinOpKind, length: Length, span: Span) {
        if let Length::Known(n) = length
            && n > 1
        {
            let message = format!(
                "`{}` applied to a length-{} operand; only the first element is used",
                op_symbol(op),
                n
            );
            self.emit(Severity::Warning, span, "RY032", message);
        }
    }
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
            let bare = crate::semantic_lists::bare_name(name);
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
        // Both `guarded` and `vector_predicate_parameter` resolve through
        // `direct_parameter`, which requires `scope.is_parameter`. So if the
        // parameter were reassigned (clearing its marker) neither would
        // return it, making the separate `is_parameter` check redundant.
        guarded
            .filter(|parameter| vector_predicate_parameter(rhs, scope) == Some(*parameter))
            .is_some()
    }
}

/// Model the base `Ops.data.frame` method without losing the table's schema.
/// Comparisons produce a logical matrix-like object, for which opaque is the
/// least misleading v1 representation. Arithmetic keeps the frame shape for
/// a scalar counterpart; otherwise it retains column names but not column
/// element types.
fn data_frame_binop_result(op: BinOpKind, lhs: &RType, rhs: &RType) -> Option<RType> {
    let is_compare = is_comparison(op);
    let is_logic = matches!(
        op,
        BinOpKind::And | BinOpKind::AndAnd | BinOpKind::Or | BinOpKind::OrOr
    );
    if !(is_compare || is_logic || op.is_arithmetic()) {
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

/// Records `expr`'s name only when it is a plain identifier binding;
/// complex targets (`d$k <- v`, `m[i] <- v`) bind no name at the
/// target itself.
fn insert_bound_name(expr: &Expr, names: &mut HashSet<String>) {
    if let Expr::Ident { name, .. } = expr {
        names.insert(name.clone());
    }
}

/// Records names bound by assignments nested anywhere in a condition
/// expression: the identifier LHS of expression-position `<-`/`<<-`
/// (descending into both operands, so a complex target like
/// `m[i <- f()]` still records `i`), and inside `{ ... }` value blocks,
/// plain assignments of any target shape (descending into the value)
/// plus bare expressions. Skips assignment targets, function bodies,
/// and the remaining statement forms (if, while, for, function
/// definitions, return), which cannot bind a name in the current
/// environment from inside a condition value.
fn collect_condition_assignment_names(expr: &Expr, names: &mut HashSet<String>) {
    let _ = walk_expr(
        expr,
        Walk {
            assign_targets: false,
            // The `<-`/`<<-` LHS is walked, not pruned: base recursed
            // into it unconditionally.
            assign_operands: true,
            fn_bodies: false,
            ..Walk::ALL
        },
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Expr(Expr::BinOp {
                    op: BinOpKind::Assign | BinOpKind::SuperAssign,
                    lhs,
                    ..
                }) => insert_bound_name(lhs, names),
                // Any target shape: a complex target (`d$k <- v`) binds
                // no name at the target, but its value still can.
                AstNode::Stmt(Stmt::Assign { target, .. }) => {
                    insert_bound_name(target, names);
                }
                // Inside a `{ ... }` value, only assignments and bare
                // expressions can bind a name in the current
                // environment; control flow, definitions, and returns
                // cannot.
                AstNode::Stmt(
                    Stmt::If { .. }
                    | Stmt::While { .. }
                    | Stmt::For { .. }
                    | Stmt::FunctionDef { .. }
                    | Stmt::Return { .. },
                ) => return ControlFlow::Continue(Descend::Skip),
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}

/// Pins the traversal shape of [`collect_condition_assignment_names`]
/// to the hand-rolled recursion it replaced: short-circuit `&&`/`||`
/// operands are walked like base did, including `{ ... }` value blocks
/// whose statements bind in the current environment.
#[cfg(test)]
mod collect_condition_assignment_names_tests {
    use super::*;
    use std::collections::HashSet;

    /// Collects the names bound in the RHS operand of a `flag && ...`
    /// expression -- the position `merge_condition_assignments` scans.
    fn collected(operand_src: &str) -> HashSet<String> {
        let src = format!("flag && {operand_src}\n");
        let file = crate::tests::parse_snippet("cond_assign_test.R", &src);
        let [Stmt::Expr(Expr::BinOp { rhs, .. })] = file.stmts.as_slice() else {
            panic!("test source must be a single `flag && ...` expression");
        };
        let mut names = HashSet::new();
        collect_condition_assignment_names(rhs, &mut names);
        names
    }

    fn assert_exact(operand_src: &str, expected: &[&str]) {
        let found = collected(operand_src);
        let expected: HashSet<String> = expected.iter().map(|name| name.to_string()).collect();
        assert_eq!(found, expected, "names from operand `{operand_src}`");
    }

    /// Assignments nested in an `&&`/`||` operand's value block bind in
    /// the current environment: both the identifier target `total` and
    /// the expression-position `delta` inside its value.
    #[test]
    fn records_assignments_inside_condition_value_blocks() {
        assert_exact(
            "({ total <- total + (delta <- f()); total })",
            &["total", "delta"],
        );
    }

    /// A non-identifier statement target (`d$k <- ...`) binds nothing
    /// itself, but its value still can: `y` must be recorded. A
    /// wildcard `Stmt(_) => Skip` callback arm pruned the whole
    /// statement -- the review blocker this pins.
    #[test]
    fn records_value_assignments_behind_complex_statement_targets() {
        assert_exact("{ d$k <- (y <- 1); TRUE }", &["y"]);
    }

    /// A bare expression statement inside a condition value block can
    /// itself carry an expression-position assignment.
    #[test]
    fn records_bare_expression_assignments_inside_condition_blocks() {
        assert_exact("{ (z <- 2); TRUE }", &["z"]);
    }

    /// The left operand of expression-position `<-`/`<<-` is walked
    /// (base recursed into it unconditionally), so a complex target
    /// like `m[i <- compute()]` still records `i`.
    #[test]
    fn walks_expression_position_assignment_lhs() {
        assert_exact("(m[i <- compute()] <- v)", &["i"]);
    }

    /// Negative controls: calls and index arguments are walked, while
    /// control statements inside value blocks stay pruned.
    #[test]
    fn prunes_control_statements_but_not_call_arguments() {
        assert_exact("g(a <- 1)", &["a"]);
        assert_exact("m[b <- 1]", &["b"]);
        assert_exact("{ for (q in 1:3) { w <- 1 }; TRUE }", &[]);
    }
}
