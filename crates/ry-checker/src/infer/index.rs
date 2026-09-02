use super::*;

fn atomic_mode(member: &RType) -> bool {
    matches!(
        member.mode,
        Mode::Integer | Mode::Double | Mode::Character | Mode::Logical | Mode::Complex | Mode::Raw
    ) && member.columns.is_none()
}

/// The conservative `$`/`[[` fallback when no schema resolves the access:
/// for list-like bases return opaque since the element type is unknowable;
/// for other types return a length-1 value of the base mode. A union base
/// would build a malformed union, so it degrades to opaque.
fn conservative_element_type(bt: &RType) -> RType {
    if matches!(
        bt.mode,
        Mode::List | Mode::Opaque | Mode::Function | Mode::Union
    ) {
        RType::unknown()
    } else {
        RType::new(bt.mode, Length::One)
    }
}

fn dollar_receiver_is_definitely_atomic(receiver: &RType) -> bool {
    match receiver.mode {
        Mode::Union => receiver
            .members
            .as_ref()
            .is_some_and(|members| !members.is_empty() && members.iter().all(atomic_mode)),
        _ => atomic_mode(receiver),
    }
}

/// A human-readable description of the receiver's mode(s) for the RY061
/// message. For a single type this is just the mode name; for a union
/// the member modes are listed so the user can see which types combined.
fn dollar_receiver_mode_description(receiver: &RType) -> String {
    if receiver.mode == Mode::Union {
        if let Some(members) = &receiver.members {
            let modes: Vec<String> = members.iter().map(|m| m.mode.to_string()).collect();
            return modes.join("` or `");
        }
    }
    receiver.mode.to_string()
}

impl Checker {
    /// Resolve the type of a subset/extract expression given the base
    /// type, the kind of index (`[`, `[[`, `$`), and the (already
    /// lowered) argument list.
    ///
    /// * `df$col` (`Dollar`): the column name lives on `args[0].name`.
    ///   With a column schema, return that column's type (RY060 on a
    ///   miss); without one, degrade conservatively: opaque for
    ///   list-like bases, else a length-1 value of `bt`'s mode.
    /// * `df[["col"]]` (`Double`): same idea, but the name comes from a
    ///   string-literal positional argument. Non-string-literal args
    ///   fall through to the conservative default.
    /// * `df[i]` or `df[i, j]` (`Single`): two-index selection on a
    ///   schema'd frame resolves the column's type (`drop = FALSE`
    ///   yields a one-column frame); otherwise returns `bt`.
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
                if dollar_receiver_is_definitely_atomic(&bt) {
                    self.emit(
                        Severity::Error,
                        span,
                        "RY061",
                        format!(
                            "$ operator is invalid for atomic vectors of mode `{}`",
                            dollar_receiver_mode_description(&bt)
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
                // No schema (or column not found after RY060): the
                // conservative default.
                conservative_element_type(&bt)
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
                    return conservative_element_type(&bt);
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
                conservative_element_type(&bt)
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
                    self.infer_args_for_diagnostics(args, scope);
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
                    if !drop_false && is_non_negative_scalar_index(&column_arg.value) {
                        return RType::unknown();
                    }
                    return bt;
                }
                if matches!(bt.mode, Mode::List) && args.len() >= 2 {
                    if let Some(column) = args.iter().find_map(|arg| match &arg.value {
                        Expr::String(column, _) => Some(column),
                        _ => None,
                    }) {
                        self.infer_args_for_diagnostics(args, scope);
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
                // For one-dimensional vector subsetting, result length is
                // controlled by the index rather than the source. Logical
                // masks select by their TRUE count, and numeric indices may
                // exclude or select nothing, so retain a length only when R's
                // index mode makes that length provable.
                let index_types: Vec<_> = args
                    .iter()
                    .map(|argument| self.infer(&argument.value, scope))
                    .collect();
                let vector_base = matches!(
                    bt.mode,
                    Mode::Integer
                        | Mode::Double
                        | Mode::Character
                        | Mode::Logical
                        | Mode::Complex
                        | Mode::Raw
                        | Mode::List
                );
                if vector_base && args.len() == 1 {
                    let index = &index_types[0];
                    let length = match index.mode {
                        Mode::Character => index.length,
                        Mode::Integer | Mode::Double if positive_numeric_index(&args[0].value) => {
                            index.length
                        }
                        Mode::Integer | Mode::Double => {
                            literal_negative_exclusion_length(bt.length, &args[0].value)
                                .unwrap_or(Length::Unknown)
                        }
                        _ => Length::Unknown,
                    };
                    let mut result = RType { length, ..bt };
                    // Generic `[` changes which names/elements are present.
                    // Until we transform ColumnSchema by the index itself,
                    // retaining the source schema would expose fields that the
                    // subset may not contain (e.g. list(a=1, b=2)[2]$a).
                    result.columns = None;
                    return result;
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
/// negative exclusion selector. Zero selects no elements under `[`, so only a
/// syntactically positive numeric literal proves scalar result length. A
/// scalar identifier has unknown sign and is therefore not sufficient.
pub(crate) fn is_non_negative_scalar_index(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(index, _) => *index > 0,
        Expr::Double(index, _) => *index > 0.0,
        Expr::String(_, _) => true,
        _ => false,
    }
}

/// Numeric subsetting preserves index length only when every index value is
/// provably positive and non-zero. The AST currently retains concrete values
/// for literals and `c(...)`; identifiers carry mode and length but no sign.
fn positive_numeric_index(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(index, _) => *index > 0,
        Expr::Double(index, _) => index.is_finite() && *index > 0.0 && index.fract() == 0.0,
        Expr::Call { func, args, .. } if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "c") => {
            !args.is_empty()
                && args
                    .iter()
                    .all(|argument| positive_numeric_index(&argument.value))
        }
        _ => false,
    }
}

/// Exact result length for a single literal negative exclusion. R ignores an
/// out-of-range exclusion; an in-range exclusion removes exactly one element.
/// Dynamic or compound negative indices remain unknown until the AST/value
/// model can prove uniqueness and bounds for every excluded position.
fn literal_negative_exclusion_length(base: Length, expr: &Expr) -> Option<Length> {
    let Expr::UnaryOp {
        op: UnaryOpKind::Neg,
        expr,
        ..
    } = expr
    else {
        return None;
    };
    let excluded = match expr.as_ref() {
        Expr::Integer(index, _) if *index > 0 => *index as usize,
        Expr::Double(index, _) if index.is_finite() && *index > 0.0 && index.fract() == 0.0 => {
            *index as usize
        }
        _ => return None,
    };
    let base = match base {
        Length::Zero => 0,
        Length::One => 1,
        Length::Known(length) => length,
        Length::Unknown => return None,
    };
    let length = base - usize::from(excluded <= base);
    Some(match length {
        0 => Length::Zero,
        1 => Length::One,
        length => Length::Known(length),
    })
}

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
///
/// This list deliberately covers a different set than
/// [`crate::semantic_lists::OPERATORS`]: RY010 suppression wants every
/// plain operator token (Logic, assignment, sequence, and access
/// operators included), while operator S3 dispatch is modeled only for
/// the Arith + Compare members (see [`is_operator_generic`]). The
/// `%`-wrapped operators (`%%`, `%/%`, user-defined `%foo%`) are absent
/// because the call site tests `contains('%')` before consulting this
/// predicate, so the two lists must not be unified.
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
                    Expr::BinOp { op, .. } => is_comparison(*op) || matches!(op, BinOpKind::In),
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

/// Functions whose arguments are bare symbols (NSE), not expressions.
/// When these are called, the checker does NOT evaluate the arguments
/// as variable references, preventing spurious RY010 warnings.
///
/// This is the FALLBACK half of the NSE knowledge. The stub-driven half
/// is the per-signature `eval` metadata in the typeshed: a function
/// whose stub declares `quoted_expression`, `captures_promise`,
/// `quoted_symbol`, `data_mask`, or `tidy_select` parameters reaches
/// that metadata only without an entry here — `is_nse_symbol_fn`
/// intercepts before signature resolution and shadows the stub. Add a
/// name here only when no stub declares its evaluation mode. The guard
/// test `nse_symbol_fallback_does_not_overlap_stub_eval_modes` fails
/// when a member gains stub coverage, so the two halves cannot drift
/// into silent overlap.
///
/// Stub coverage is genuinely absent for every member (issue #41):
///   * base: `quote`, `substitute`, `bquote`, and `delayedAssign` have
///     stubs without `eval` fields; `makeActiveBinding` has no stub.
///   * rlang: the kept names have stubs without `eval` fields.
///   * ggplot2 and data.table ship no stubs.
///   * tidyselect's stub does not declare `peek_vars`. `all_vars` is
///     not here: dplyr — the package it is called through — declares
///     `expr: data_mask` for it.
pub(crate) const NSE_SYMBOL_FNS: &[&str] = &[
    // ggplot2 NSE
    "from_theme",
    "aes",
    "aes_",
    "aes_string",
    "aes_q",
    // rlang NSE
    "sym",
    "expr",
    "exprs",
    "quo",
    "abort",
    "inform",
    "defuse",
    "tidyeval_data",
    "new_formula",
    "new_quosure",
    // tidyselect package functions
    "peek_vars",
    // base NSE helpers
    "quote",
    "substitute",
    "bquote",
    "delayedAssign",
    "makeActiveBinding",
    // data.table NSE
    "setkey",
    "setkeyv",
    "setindex",
    "setindexv",
];

pub(crate) fn is_nse_symbol_fn(name: &str) -> bool {
    let name = crate::semantic_lists::bare_name(name);
    NSE_SYMBOL_FNS.contains(&name)
}

pub(crate) fn is_dplyr_control_arg(name: &str) -> bool {
    matches!(
        name,
        ".by" | ".groups" | ".keep" | ".before" | ".after" | ".drop"
    )
}

/// Whether `name` is an operator that ry models as an S3 generic, e.g. the
/// `+` in `` `+.widget` ``. This is exactly the Arith + Compare operator
/// set registered as [`crate::semantic_lists::OPERATORS`] and already used
/// by the S3 method-name splitter, so the predicate reads that constant
/// rather than restating the symbols; the two users cannot drift apart.
///
/// Membership is pinned to R's own Arith and Compare group definitions by
/// the oracle test in `tests/semantic_lists.rs`. Logic and other operator
/// tokens are deliberately outside the set: they are RY010-suppression
/// operator symbols (see [`is_operator_symbol`]), not modeled generics.
pub(crate) fn is_operator_generic(name: &str) -> bool {
    crate::semantic_lists::OPERATORS.contains(&name)
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

#[cfg(test)]
mod operator_generic_tests {
    use super::is_operator_generic;
    use crate::semantic_lists::OPERATORS;

    /// `is_operator_generic` must recognize exactly the members of
    /// `semantic_lists::OPERATORS`: the same constant backs the S3
    /// method-name splitter in `lib.rs`, so any divergence between the two
    /// representations is an inconsistency (issue #42). The negative
    /// samples pin the set to the Arith + Compare members -- Logic,
    /// assignment, sequence, and access operators are operator symbols
    /// for RY010 suppression but never operator generics, `%in%` is a
    /// function-backed infix operator outside both dispatch groups, and a
    /// full method name like `+.foo` is split before this predicate runs.
    /// Reinstating a separate hardcoded symbol set here fails this test.
    #[test]
    fn operator_generic_recognizes_exactly_the_operators_list() {
        for operator in OPERATORS {
            assert!(
                is_operator_generic(operator),
                "OPERATORS member {operator:?} must be recognized as an operator generic"
            );
        }
        for non_generic in [
            "&", "|", "&&", "||", "!", ":", "<-", "<<-", "=", "~", "$", "@", "?", "%in%", "+.foo",
        ] {
            assert!(
                !is_operator_generic(non_generic),
                "{non_generic:?} is not an Arith/Compare operator and must not be recognized"
            );
        }
    }
}
