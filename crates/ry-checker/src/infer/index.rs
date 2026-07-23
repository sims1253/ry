use super::*;

impl Checker {
    pub(crate) fn infer_index(
        &mut self,
        bt: RType,
        kind: IndexKind,
        args: &[Arg],
        span: Span,
        default_null_receiver: bool,
        scope: &mut Scope,
    ) -> RType {
        if matches!(kind, IndexKind::Dollar) {
            if let Some(class) = bt.class.first()
                && let Some(slots) = self.fn_table.s4_classes.get(class.as_ref())
            {
                let slot = args.first().and_then(|argument| argument.name.as_deref());
                return slot
                    .and_then(|slot| slots.get(slot))
                    .map(|class| RType::unknown().with_class(ClassVector::single(class)))
                    .unwrap_or_else(RType::unknown);
            }
        }
        // A parameter's NULL default describes only the omitted-argument
        // call shape. When it is the direct receiver of `$` or `[[`, callers
        // may instead provide a list-like value, so keep the access opaque.
        // Directly assigned NULL deliberately retains the normal NULL result.
        if default_null_receiver
            && matches!(kind, IndexKind::Dollar | IndexKind::Double)
            && matches!(bt.mode, Mode::Null)
        {
            return RType::unknown();
        }
        match kind {
            IndexKind::Dollar => {
                // RY061: `$` on an atomic vector is a runtime error in R
                // ("$ operator is invalid for atomic vectors"). Only flag
                // when we're confident the type is atomic (not opaque,
                // not list, not function, not NULL). List-like types
                // without a schema are fine -- the column might exist
                // dynamically -- and atomic types *with* a schema are
                // already covered by the schema lookup / RY060 below.
                if matches!(
                    bt.mode,
                    Mode::Integer
                        | Mode::Double
                        | Mode::Character
                        | Mode::Logical
                        | Mode::Complex
                        | Mode::Raw
                ) && bt.columns.is_none()
                {
                    self.emit(
                        Severity::Error,
                        span,
                        "RY061",
                        format!(
                            "$ operator is invalid for atomic vectors of mode `{}`",
                            bt.mode
                        ),
                    );
                    return RType::unknown();
                }
                // The parser records `$col` as a single arg with
                // `name = Some("col")` and a synthesized `value` of
                // `Expr::Ident { name: "col" }`. The value is NOT a
                // real expression to be inferred: doing so would emit a
                // spurious RY010 on the column name. So we deliberately
                // do not call `infer` on it.
                let col = args.first().and_then(|a| a.name.as_deref());
                if let Some(name) = col {
                    if let Some(schema) = &bt.columns {
                        if let Some(t) = schema.get(name) {
                            return t;
                        }
                        // RY060 for a `$` schema miss only on data frames.
                        // In R, `list(a=1)$missing` returns NULL (no
                        // error); only data frames make a missing `$`
                        // name a hard error worth flagging. Mirror the `[[`-with-string guard below.
                        if bt.class.contains("data.frame") && schema.complete {
                            self.emit_undefined_column(name, schema, span);
                            // Fall through to the conservative default so
                            // downstream code still has *a* type to work
                            // with after the diagnostic.
                        } else if matches!(bt.mode, Mode::List) && bt.class == ClassVector::empty()
                        {
                            // Plain list `$` miss yields NULL in R.
                            return RType::new(Mode::Null, Length::Zero);
                        }
                    }
                }
                // No schema (or column not found after RY060): for
                // list-like types, return opaque since we don't know
                // the element type. For other types, return a length-1
                // value of the base mode. A union base would build a
                // malformed union here, so degrade to opaque.
                if matches!(
                    bt.mode,
                    Mode::List | Mode::Opaque | Mode::Function | Mode::Union
                ) {
                    RType::unknown()
                } else {
                    RType::new(bt.mode, Length::One)
                }
            }
            IndexKind::Double => {
                // `df[["col"]]` or `x[[i]]`: the index can be a string
                // literal (column name) or an integer literal (positional
                // index). For string literals we look up by column name
                // ONLY on data frames (class data.frame). For plain
                // lists, string access is dynamic and we don't flag it.
                let arg_expr = args.first().map(|a| &a.value);
                if let Some(Expr::String(name, _)) = arg_expr {
                    if let Some(schema) = &bt.columns {
                        if let Some(t) = schema.get(name) {
                            return t;
                        }
                        // Only emit RY060 for data frames, not plain lists.
                        // Lists created by lapply etc. have internal
                        // [[N]] schemas; string access is dynamic.
                        if bt.class.contains("data.frame") && schema.complete {
                            self.emit_undefined_column(name, schema, span);
                        }
                    }
                    if matches!(
                        bt.mode,
                        Mode::List | Mode::Opaque | Mode::Function | Mode::Union
                    ) {
                        return RType::unknown();
                    }
                    return RType::new(bt.mode, Length::One);
                }
                // Integer or double literal index: look up `[[N]]` in
                // the schema. In R, `1` is a double, `1L` is an integer;
                // both are valid indices for `[[`, so we handle both.
                let int_idx = match arg_expr {
                    Some(Expr::Integer(i, _)) => Some(*i as f64),
                    Some(Expr::Double(f, _)) => Some(*f),
                    _ => None,
                };
                if let Some(idx) = int_idx {
                    if let Some(schema) = &bt.columns {
                        let key = format!("[[{}]]", idx as i64);
                        if let Some(t) = schema.get(&key) {
                            return t;
                        }
                        // Index not in schema: if all elements have the
                        // same type (homogeneous list from lapply etc.),
                        // return that common type. Otherwise opaque.
                        if let Some(common) = schema.homogeneous_element_type() {
                            return common;
                        }
                    }
                    // No schema or heterogeneous: opaque is safer than
                    // `bt.element()` (which returns list<1> for lists).
                    return RType::unknown();
                }
                // Non-literal arg: infer it for diagnostics, then return
                // the conservative default. A union base would build a
                // malformed union, so degrade to opaque.
                if let Some(a) = args.first() {
                    self.infer(&a.value, scope);
                }
                if let Some(schema) = &bt.columns {
                    if let Some(common) = schema.homogeneous_element_type() {
                        if !bt.class.contains("data.frame") || schema.complete {
                            return common;
                        }
                    }
                }
                if matches!(
                    bt.mode,
                    Mode::List | Mode::Opaque | Mode::Function | Mode::Union
                ) {
                    RType::unknown()
                } else {
                    RType::new(bt.mode, Length::One)
                }
            }
            IndexKind::Single => {
                // `df[i, j]` selects a column when `j` is scalar and the
                // default `drop = TRUE` is in effect.  A data frame's own
                // length is its number of columns, not its row count, so
                // returning `bt` here would make `df[, 1]` look like a
                // length-ncol vector.  Prefer the schema's column type,
                // which `infer_data_frame` has already widened to the frame
                // row count.
                if bt.class.contains("data.frame") && args.len() >= 2 {
                    let column_arg = &args[1];
                    let drop_false = args.iter().any(|arg| {
                        arg.name.as_deref() == Some("drop")
                            && matches!(arg.value, Expr::Logical(false, _))
                    });
                    let column = match &column_arg.value {
                        Expr::String(name, _) => {
                            bt.columns.as_ref().and_then(|schema| schema.get(name))
                        }
                        Expr::Integer(index, _) if *index >= 1 => bt
                            .columns
                            .as_ref()
                            .and_then(|schema| schema.columns.get(*index as usize - 1))
                            .map(|(_, ty)| ty.clone()),
                        Expr::Double(index, _) if *index >= 1.0 && index.fract() == 0.0 => bt
                            .columns
                            .as_ref()
                            .and_then(|schema| schema.columns.get(*index as usize - 1))
                            .map(|(_, ty)| ty.clone()),
                        _ => None,
                    };
                    for argument in args {
                        self.infer(&argument.value, scope);
                    }
                    if let Some(column) = column {
                        if !drop_false {
                            return column;
                        }
                        let name = match &column_arg.value {
                            Expr::String(name, _) => name.clone(),
                            _ => "[[1]]".to_string(),
                        };
                        return RType::new(Mode::List, Length::One)
                            .with_class(ClassVector::single("data.frame"))
                            .with_columns(Arc::new(ColumnSchema {
                                columns: vec![(name, column)],
                                complete: true,
                                locally_constructed: false,
                            }));
                    }
                    // A scalar but dynamic column index still drops to a
                    // vector. Its mode and row count are not knowable.
                    if !drop_false && is_non_negative_scalar_index(&column_arg.value, scope) {
                        return RType::unknown();
                    }
                    return bt;
                }
                if matches!(bt.mode, Mode::List) && args.len() >= 2 {
                    if let Some(column) = args.iter().find_map(|arg| match &arg.value {
                        Expr::String(column, _) => Some(column),
                        _ => None,
                    }) {
                        for argument in args {
                            self.infer(&argument.value, scope);
                        }
                        if let Some(schema) = &bt.columns {
                            if let Some(column_type) = schema.get(column) {
                                return column_type;
                            }
                            if !schema.complete {
                                return RType::unknown();
                            }
                        }
                        return RType::unknown();
                    }
                }
                // Single-bracket subsetting semantics are complex
                // (column slice vs row slice depends on commas and
                // drops). For atomic vectors with one scalar index,
                // however, the result is a scalar of the same mode.
                let scalar_atomic_index = args.len() == 1
                    && args
                        .first()
                        .is_some_and(|a| is_non_negative_scalar_index(&a.value, scope))
                    && matches!(
                        bt.mode,
                        Mode::Integer
                            | Mode::Double
                            | Mode::Character
                            | Mode::Logical
                            | Mode::Complex
                            | Mode::Raw
                    );
                for a in args {
                    self.infer(&a.value, scope);
                }
                if scalar_atomic_index {
                    return RType {
                        length: Length::One,
                        ..bt
                    };
                }
                bt
            }
        }
    }

    /// Emit RY060 for a column access whose name is not in the schema.
    /// Lists the first 5 available column names so the user has
    /// something to act on.
    pub(crate) fn emit_undefined_column(&mut self, col: &str, schema: &ColumnSchema, span: Span) {
        let names = schema.names();
        let preview: Vec<&str> = names.iter().take(5).cloned().collect();
        let available = if names.len() > 5 {
            format!("{}, ...", preview.join(", "))
        } else if preview.is_empty() {
            "(none)".to_string()
        } else {
            preview.join(", ")
        };
        self.emit(
            Severity::Error,
            span,
            "RY060",
            format!(
                "column `{}` not found in data frame schema; available columns: {}",
                col, available
            ),
        );
    }
}

/// Whether an index expression is a scalar element selector, rather than a
/// negative exclusion selector. R has no sign information in `RType`, so a
/// scalar identifier is accepted while syntactically negative literals are
/// deliberately rejected.
pub(crate) fn is_non_negative_scalar_index(expr: &Expr, scope: &Scope) -> bool {
    match expr {
        Expr::Integer(index, _) => *index >= 0,
        Expr::Double(index, _) => *index >= 0.0,
        Expr::String(_, _) => true,
        Expr::Ident { name, .. } => scope.get(name).is_some_and(|ty| {
            matches!(ty.length, Length::One)
                && matches!(ty.mode, Mode::Integer | Mode::Double | Mode::Character)
        }),
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            ..
        } => false,
        _ => false,
    }
}

/// Apply a `SeverityFilter` to a vec of diagnostics in place. Each
/// diagnostic's severity is replaced by the filter's effective
/// severity for its code; diagnostics for codes the filter suppresses
/// are dropped entirely.
///
/// Both `Checker::apply_filter`, `Project::apply_filter`, and the CLI
/// (for per-file diagnostic vecs produced by `Project::check`) call
/// this. Keeping the logic here avoids duplicating the resolution
/// rules.
/// Quick literal-only inference for function parameter defaults. We
/// don't have a scope yet at the point of `record_fn`, but for typed
/// defaults (`x = 1L`, `trim = 0`, `verbose = TRUE`) the literal
/// carries enough information.
pub(crate) fn infer_literal_default(e: &Expr) -> RType {
    match e {
        Expr::Logical(_, _) => RType::scalar(Mode::Logical),
        Expr::Integer(_, _) => RType::scalar(Mode::Integer),
        Expr::Double(_, _) => RType::scalar(Mode::Double),
        Expr::String(_, _) => RType::scalar(Mode::Character),
        Expr::Null(_) => RType::new(Mode::Null, Length::Zero),
        Expr::Na(t, _) => t.clone(),
        // Anything more complex (call, ident, binop) needs a scope; defer
        // to the first fixpoint iteration by starting as UNKNOWN.
        _ => RType::unknown(),
    }
}

/// True if `e` is syntactically a `return(...)` or `invisible(...)` call.
pub(crate) fn is_return_call(e: &Expr) -> bool {
    matches!(e, Expr::Call { func, .. }
        if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "return" || name == "invisible"))
}

/// True if the string is an R operator symbol that might be referenced
/// as a (possibly backtick-quoted) identifier, e.g. `+`, `*`, `<-`.
/// These are commonly user-defined or package-imported operators that
/// the checker cannot resolve against any scope, typeshed, or FnTable.
/// Used to suppress spurious RY010 (unbound variable) on such names.
pub(crate) fn is_operator_symbol(s: &str) -> bool {
    matches!(
        s,
        "+" | "-"
            | "*"
            | "/"
            | "^"
            | "<"
            | ">"
            | "<="
            | ">="
            | "=="
            | "!="
            | "&"
            | "|"
            | "&&"
            | "||"
            | "!"
            | ":"
            | "<-"
            | "<<-"
            | "="
            | "~"
            | "$"
            | "@"
            | "?"
    )
}

pub(crate) fn span_of(e: &Expr) -> Span {
    match e {
        Expr::Logical(_, s) => *s,
        Expr::Integer(_, s) => *s,
        Expr::Double(_, s) => *s,
        Expr::String(_, s) => *s,
        Expr::Null(s) => *s,
        Expr::Na(_, s) => *s,
        Expr::Ident { span, .. } => *span,
        Expr::Call { span, .. } => *span,
        Expr::BinOp { span, .. } => *span,
        Expr::UnaryOp { span, .. } => *span,
        Expr::Index { span, .. } => *span,
        Expr::Function { span, .. } => *span,
        Expr::Block { span, .. } => *span,
        Expr::If { span, .. } => *span,
        Expr::Unknown(s) => *s,
    }
}

/// Whether a condition expression is the idiomatic numeric-truthiness
/// non-empty check: a direct call to `length`, `nrow`, or `ncol` via a bare
/// identifier callee (any args). These return an integer length-1, which R
/// silently coerces to logical in `if`/`while` -- but `if (length(x))` /
/// `if (nrow(df))` are so idiomatic in real R code that the RY003 coercion
/// info is pure noise there. We suppress ONLY that numeric-truthiness arm
/// for this shape; a genuinely wrong condition (e.g. `if (1L)`) still emits
/// the informational diagnostic.
///
/// Negation (`if (!length(x))`) is deliberately out of scope: it is typed
/// through the unary `!` operator, not this call shape.
pub(crate) fn is_numeric_truthiness_idiom(cond: &Expr, scope: &Scope) -> bool {
    if let Expr::Call { func, args, .. } = cond {
        if let Expr::Ident { name, .. } = func.as_ref() {
            if matches!(name.as_str(), "length" | "nrow" | "ncol" | "NROW" | "NCOL") {
                return true;
            }
            if name == "sum" {
                return args.first().is_some_and(|argument| match &argument.value {
                    Expr::Ident { name, .. } => scope
                        .get(name)
                        .is_some_and(|ty| matches!(ty.mode, Mode::Logical)),
                    Expr::BinOp { op, .. } => matches!(
                        op,
                        BinOpKind::Lt
                            | BinOpKind::Le
                            | BinOpKind::Gt
                            | BinOpKind::Ge
                            | BinOpKind::Eq
                            | BinOpKind::Ne
                            | BinOpKind::In
                    ),
                    Expr::Call { func, .. } => {
                        ident_name(func).is_some_and(|predicate| predicate.starts_with("is."))
                    }
                    _ => false,
                });
            }
        }
    }
    false
}

/// RY040's missing-list-field case is intentionally limited to a complete
/// schema built by a local `list(...)` expression.  Imported data-frame
/// schemas and transformed/narrowed values can look equally complete, but
/// their absent fields are not strong enough evidence for an arithmetic
/// diagnostic.
pub(crate) fn known_null_arithmetic_operand(expr: &Expr, scope: &Scope) -> bool {
    if matches!(expr, Expr::Null(_)) {
        return true;
    }
    let Expr::Index {
        base, kind, args, ..
    } = expr
    else {
        return false;
    };
    let Some(field) = assigned_column_name(*kind, args) else {
        return false;
    };
    let Expr::Ident { name, .. } = base.as_ref() else {
        return false;
    };
    scope
        .get(name)
        .and_then(|ty| ty.columns.as_ref())
        .is_some_and(|schema| {
            schema.locally_constructed && schema.complete && schema.get(field).is_none()
        })
}

/// Extract an integer value from a literal expression. Returns
/// `Some(n)` for `Expr::Integer(n, _)` and for `Expr::Double(f, _)`
/// when `f` is a finite whole number (e.g. `2.0`). Returns `None` for
/// non-literal expressions, NaN/Inf, or fractional doubles.
///
/// Used by the literal-based length inference paths (`:` colon
/// operator, `rep`, `seq`) to compute exact result lengths when the
/// relevant arguments are literal integers or whole-number doubles.
/// We look at the raw AST rather than the inferred `RType` because the
/// type lattice discards the runtime value (it only carries mode and
/// length).
pub(crate) fn extract_literal_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(n, _) => Some(*n),
        Expr::Double(f, _) if f.is_finite() && f.fract() == 0.0 => Some(*f as i64),
        _ => None,
    }
}

/// True if `e` is a magrittr (`.`) or base-R (`_`) pipe placeholder.
/// These are bare identifier references used inside a piped call to
/// mark where the LHS value should be substituted.
pub(crate) fn is_pipe_placeholder(e: &Expr) -> bool {
    matches!(e, Expr::Ident { name, .. } if name == "." || name == "_")
}

/// Functions whose arguments are bare symbols (NSE), not expressions.
/// When these are called, the checker does NOT evaluate the arguments
/// as variable references, preventing spurious RY010 warnings.
///
/// Includes popular package functions commonly used in NSE contexts:
///   * ggplot2: from_theme, aes, aes_, aes_string, aes_q
///   * rlang: sym, expr, quo, and other helpers with symbol arguments
///   * base: quote, substitute, bquote, alist (already in typeshed but also
///     used as NSE)
pub(crate) fn is_nse_symbol_fn(name: &str) -> bool {
    let name = name.rsplit_once("::").map(|(_, n)| n).unwrap_or(name);
    matches!(
        name,
        // ggplot2 NSE
        "from_theme" | "aes" | "aes_" | "aes_string" | "aes_q"
        // rlang NSE
        | "sym" | "expr"
        | "exprs" | "quo" | "quos" | "abort" | "inform"
        | "defuse" | "tidyeval_data" | "new_formula" | "new_quosure"
        // dplyr/tidyselect NSE
        | "tidyselect" | "all_vars" | "peek_vars"
        // Common NSE helpers
        | "quote" | "substitute" | "bquote" | "alist" | "delayedAssign" | "makeActiveBinding"
        // data.table NSE
        | "setkey" | "setkeyv" | "setindex" | "setindexv"
    )
}

pub(crate) fn is_dplyr_control_arg(name: &str) -> bool {
    matches!(
        name,
        ".by" | ".groups" | ".keep" | ".before" | ".after" | ".drop"
    )
}

pub(crate) fn is_operator_generic(name: &str) -> bool {
    matches!(
        name,
        "+" | "-" | "*" | "/" | "^" | "%%" | "%/%" | "==" | "!=" | "<" | "<=" | ">" | ">="
    )
}

pub(crate) fn insert_s3_dispatch_context(method_name: &str, scope: &mut Scope, globals: &Globals) {
    let method_name = semantic_argument_name(method_name);
    let group_method = split_s3_method_name(&method_name, globals).is_some_and(|(generic, _)| {
        matches!(generic.as_str(), "Ops" | "Math" | "Summary" | "matrixOps")
    });
    if group_method {
        scope.insert(".Generic", RType::scalar(Mode::Character));
        scope.insert(".Method", RType::new(Mode::Character, Length::Unknown));
        scope.insert(".Class", RType::new(Mode::Character, Length::Unknown));
        scope.insert(".Group", RType::scalar(Mode::Character));
    }
}

pub(crate) fn assigned_names_in_body(body: &[Stmt]) -> HashSet<String> {
    fn visit(statement: &Stmt, names: &mut HashSet<String>) {
        match statement {
            Stmt::Assign { target, value, .. } => {
                if let Expr::Ident { name, .. } = target {
                    names.insert(name.clone());
                }
                // A nested closure has its own locals; do not leak them into
                // the enclosing closure's capture candidates.
                if !matches!(value, Expr::Function { .. }) {
                    visit_expr(value, names);
                }
            }
            Stmt::If { then, else_, .. } => {
                for statement in then {
                    visit(statement, names);
                }
                if let Some(else_) = else_ {
                    for statement in else_ {
                        visit(statement, names);
                    }
                }
            }
            Stmt::For { name, body, .. } => {
                names.insert(name.clone());
                for statement in body {
                    visit(statement, names);
                }
            }
            Stmt::While { body, .. } => {
                for statement in body {
                    visit(statement, names);
                }
            }
            Stmt::FunctionDef { name, .. } => {
                if let Some(name) = name {
                    names.insert(name.clone());
                }
            }
            Stmt::Expr(expr) => visit_expr(expr, names),
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    visit_expr(value, names);
                }
            }
        }
    }
    fn visit_expr(expr: &Expr, names: &mut HashSet<String>) {
        match expr {
            Expr::BinOp {
                op: BinOpKind::Assign | BinOpKind::SuperAssign,
                lhs,
                rhs,
                ..
            } => {
                if let Expr::Ident { name, .. } = lhs.as_ref() {
                    names.insert(name.clone());
                }
                visit_expr(rhs, names);
            }
            Expr::Block { body, .. } => {
                for statement in body {
                    visit(statement, names);
                }
            }
            Expr::If { then, else_, .. } => {
                visit_expr(then, names);
                if let Some(else_) = else_ {
                    visit_expr(else_, names);
                }
            }
            _ => {}
        }
    }

    let mut names = HashSet::new();
    for statement in body {
        visit(statement, &mut names);
    }
    names
}
