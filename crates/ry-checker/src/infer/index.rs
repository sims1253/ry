use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmts};
use std::ops::ControlFlow;

fn atomic_mode(member: &RType) -> bool {
    matches!(
        member.mode,
        Mode::Integer | Mode::Double | Mode::Character | Mode::Logical | Mode::Complex | Mode::Raw
    ) && member.columns.is_none()
}

/// Whether a `[` receiver is table-shaped, making its index arguments
/// data-masked positions (data.table semantics): free names resolve
/// against the receiver's columns before scope functions, and a
/// receiver without usable column knowledge keeps bare names opaque
/// rather than borrowing a scope function's type (#369). Only
/// data.table's `[` masks its arguments: a receiver classed
/// `data.table`, or an opaque value — data.table ships no stubs, so
/// runtime data.tables built by `as.data.table()` / `fread()` / package
/// data type as opaque (#369's corpus shape). Base R does not data-mask
/// `[`: `[.data.frame` evaluates `i`/`j` as ordinary promises in the
/// calling frame (R-lang §2.1.8), so class-`data.frame` receivers,
/// plain lists, atomic vectors, and matrices evaluate their indices
/// eagerly and keep the eager diagnostics — `flights[month == 6L]` on a
/// base data.frame errors at runtime when `month` is a closure.
fn table_index_receiver(bt: &RType) -> bool {
    !bt.class.contains("matrix") && (bt.class.contains("data.table") || bt.mode == Mode::Opaque)
}

/// A light table mask: the receiver is opaque without column knowledge,
/// so the mask is enumerable-free but keeps names eager (see
/// [`Checker::table_index_mask_scope`]). Named `[` arguments there can
/// only be a table method's controls, never base-vector subsetting.
fn light_table_mask(scope: &Scope) -> bool {
    !scope.data_mask_unknown
        && scope.get(crate::nse::DATA_MASK_ACTIVE).is_some()
        && scope.get(crate::nse::DATA_MASK_COLUMNS_FIRST).is_some()
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

pub(super) fn dollar_atomic_receiver_may_dispatch(receiver: &RType) -> bool {
    if receiver.mode == Mode::Union {
        // structure() can attach the dispatch class to the union itself.
        return receiver.class != ClassVector::empty()
            || receiver
                .members
                .as_ref()
                .is_some_and(|members| members.iter().any(dollar_atomic_receiver_may_dispatch));
    }
    atomic_mode(receiver) && receiver.class != ClassVector::empty()
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
            IndexKind::Slot => {
                // `.Data` access can be valid even for atomic receivers.
                // Pooled S4 declarations do not prove the result. Keep the
                // existing call-effects policy; explicit accessor overrides
                // are handled before receiver inference. Slot names are data.
                RType::unknown()
            }
            IndexKind::Dollar => {
                // `$` dispatches on classed atomic values. The pooled method
                // table cannot prove which method applies here, its return
                // type, or its writes to the caller.
                if dollar_atomic_receiver_may_dispatch(&bt) {
                    scope.invalidate_unknown_effects();
                    return RType::unknown();
                }
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
                // `x[i]` / `x[i, j]` on a data.table-shaped receiver (a
                // `data.table` class or the opaque value an unstubbed
                // data.table call types as) evaluates its index arguments
                // in a data mask over the receiver's columns (#369): a
                // column named like a scope function (`month`, `table`,
                // `count`) resolves as a column, and a receiver without
                // usable column knowledge keeps bare names opaque rather
                // than borrowing a scope function's type. Base data.frame
                // receivers, plain lists, and atomic receivers keep eager
                // index evaluation — base R does not data-mask `[`.
                let mut table_mask =
                    table_index_receiver(&bt).then(|| self.table_index_mask_scope(scope, &bt));
                let index_scope: &mut Scope = match table_mask.as_mut() {
                    Some(mask) => mask,
                    None => scope,
                };
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
                    let _ = self.infer_table_index_args(args, &bt, index_scope);
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
                        let _ = self.infer_table_index_args(args, &bt, index_scope);
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
                let index_types = self.infer_table_index_args(args, &bt, index_scope);
                if let Some(members) = &bt.members {
                    if args.len() != 1
                        || members
                            .iter()
                            .any(|member| !member.class.known || member.class.len > 0)
                    {
                        return RType::unknown();
                    }
                    return members
                        .iter()
                        .map(|member| {
                            subset_vector(member, &index_types[0], &args[0].value)
                                .unwrap_or_else(RType::unknown)
                        })
                        .reduce(RType::join)
                        .unwrap_or_else(RType::unknown);
                }
                if args.len() == 1
                    && let Some(result) = subset_vector(&bt, &index_types[0], &args[0].value)
                {
                    return result;
                }
                if bt.mode == Mode::Opaque {
                    RType::unknown()
                } else {
                    bt
                }
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

    /// Infer every `[` index argument through
    /// [`Checker::infer_table_index_argument`], recording the receiver's
    /// type and each argument's effective `[.data.table` role on the
    /// scope (see [`SelectSubscript`]; issue #367) so the unary-operator
    /// diagnostics recognize data.table select forms (`dt[, -c("col")]`,
    /// `dt[, !c("col")]`, `dt[!"key"]`, `dt[!list()]`) while the
    /// mask-aware argument walk runs. The role comes from the argument's
    /// name when given (`j = `, `.SDcols = `) and from its positional
    /// slot otherwise. The previous context is saved and restored around
    /// every argument, so nested subscripts inside a selector do not
    /// leak their receiver into sibling arguments.
    fn infer_table_index_args(
        &mut self,
        args: &[Arg],
        receiver: &RType,
        scope: &mut Scope,
    ) -> Vec<RType> {
        let mut types = Vec::with_capacity(args.len());
        for (slot, argument) in args.iter().enumerate() {
            let previous = scope.select_subscript.take();
            scope.select_subscript = Some(SelectSubscript {
                receiver: receiver.clone(),
                role: SelectSlotRole::resolve(slot, argument.name.as_deref()),
            });
            let ty = self.infer_table_index_argument(argument, scope);
            scope.select_subscript = previous;
            types.push(ty);
        }
        types
    }

    /// Infer one `[` index argument. data.table's `:=` column assignment
    /// names its targets on the call's left side -- a bare symbol, a
    /// `c()` of names, or named arguments in the functional
    /// `` `:=`(col = value) `` form -- so those names denote columns to
    /// create or replace. They are targets, not references: they must
    /// not resolve to scope functions or fire RY010 (#369). `:=` is not
    /// defined outside data.table, so this reading applies only while a
    /// table mask is active. A named argument of a light mask belongs to
    /// a table method's controls (`by`, `.SDcols`, `drop`, ...);
    /// base-vector subsetting has none, so its value resolves through an
    /// unenumerable mask. Every other argument is an ordinary expression
    /// of the surrounding mask.
    fn infer_table_index_argument(&mut self, argument: &Arg, scope: &mut Scope) -> RType {
        if let Expr::Call { func, args, .. } = &argument.value
            && matches!(
                func.as_ref(),
                Expr::Ident { name, .. } if name == ":=" || name == "`:=`"
            )
            && scope.get(crate::nse::DATA_MASK_ACTIVE).is_some()
        {
            // Infix shape `target := value`, also spelled `` `:=`(target, value) ``.
            if args.len() == 2 && args.iter().all(|operand| operand.name.is_none()) {
                self.check_table_assign_target(&args[0].value, scope);
                return self.infer(&args[1].value, scope);
            }
            // Functional shape `` `:=`(col = value, ...) ``: every formal
            // name is a column target; only the values are expressions.
            if !args.is_empty() && args.iter().all(|operand| operand.name.is_some()) {
                let mut result = RType::unknown();
                for operand in args {
                    result = self.infer(&operand.value, scope);
                }
                return result;
            }
        } else if argument.name.is_some() && light_table_mask(scope) {
            let mut control = scope.independent_execution_scope().with_unknown_data_mask();
            // The control scope isolates name resolution only — the named
            // argument's value is still the syntactic operand of the
            // in-flight `[` call (`.SDcols = !c("a")` keeps its select
            // form), so the subscript context rides along explicitly.
            control.select_subscript = scope.select_subscript.clone();
            return self.infer(&argument.value, &mut control);
        }
        self.infer(&argument.value, scope)
    }

    /// A `:=` target is a column name, not a reference: bare symbols and
    /// a `c()`/`list()` of string literals name columns directly, while
    /// any other shape is a computed target that evaluates in the mask.
    fn check_table_assign_target(&mut self, target: &Expr, scope: &mut Scope) {
        let names_only = matches!(target, Expr::Ident { .. })
            || matches!(
                target,
                Expr::Call { func, args, .. }
                    if matches!(
                        func.as_ref(),
                        Expr::Ident { name, .. } if name == "c" || name == "list"
                    ) && args
                        .iter()
                        .all(|operand| matches!(operand.value, Expr::String(_, _)))
            );
        if !names_only {
            let _ = self.infer(target, scope);
        }
    }
}

/// Whether a unary operator applied to `operand` inside a `[` subscript
/// argument is a documented data.table select form rather than a base-R
/// operand error (issue #367).
///
/// `[.data.table` interprets `-<character>` and `!<character>` in the
/// column-selector (`j`) argument as column drops, `!<character>` /
/// `!<list>` in the row-filter (`i`) argument as key exclusion /
/// not-join, and both `-<character>` and `!<character>` on `.SDcols` as
/// selection inversion. Base R has no negative or negated character
/// subscript: `v[-c("a")]` and `v[!"a"]` error with "invalid argument
/// to unary operator" / "invalid argument type" (oracle-verified), so
/// the forms are admitted only when the receiver is not provably a base
/// object — data.table ships no stubs, so its receivers are opaque to
/// inference, as are parameters flowing into package code.
pub(crate) fn select_subscript_form(
    context: Option<&SelectSubscript>,
    op: UnaryOpKind,
    operand: Mode,
) -> bool {
    let Some(context) = context else {
        return false;
    };
    if base_subscript_receiver(&context.receiver) {
        return false;
    }
    match (op, context.role) {
        // `-<character>` is a documented column drop in `j` and a
        // documented `.SDcols` inversion; data.table gives it no select
        // meaning in `i` or any other argument.
        (UnaryOpKind::Neg, SelectSlotRole::J | SelectSlotRole::Sdcols) => {
            operand == Mode::Character
        }
        // `!<character>` / `!<list>` selects keys/rows in `i` and drops
        // columns in `j`; `.SDcols` takes the same character inversion
        // but no list form.
        (UnaryOpKind::Not, SelectSlotRole::I | SelectSlotRole::J) => {
            matches!(operand, Mode::Character | Mode::List)
        }
        (UnaryOpKind::Not, SelectSlotRole::Sdcols) => operand == Mode::Character,
        _ => false,
    }
}

/// Whether `receiver` is provably an object whose `[` follows base R
/// subscript rules and therefore cannot interpret data.table select
/// forms: a classless atomic vector or list, or a plain `data.frame`.
/// Anything else stays quiet — opaque values, unions with a non-base
/// member, and any classed receiver other than a plain `data.frame`:
/// a known class such as `Date` has no modeled `[` method here, and an
/// unmodeled class dispatch may give `-`/`!` subscripts their own
/// meaning.
fn base_subscript_receiver(receiver: &RType) -> bool {
    if receiver.class.contains("data.table") {
        return false;
    }
    let classless = receiver.class.known && receiver.class.len == 0;
    match receiver.mode {
        Mode::Integer
        | Mode::Double
        | Mode::Logical
        | Mode::Complex
        | Mode::Raw
        | Mode::Character => classless,
        // Plain lists and base data.frames: a negative or negated
        // character subscript is an error in R.
        Mode::List => classless || receiver.class == ClassVector::single("data.frame"),
        // NULL and functions are never data.table receivers.
        Mode::Null | Mode::Function => true,
        Mode::Opaque => false,
        Mode::Union => receiver.members.as_ref().is_some_and(|members| {
            !members.is_empty() && members.iter().all(base_subscript_receiver)
        }),
    }
}

fn subset_vector(base: &RType, index: &RType, expression: &Expr) -> Option<RType> {
    if base.mode == Mode::Null {
        return Some(base.clone());
    }
    if !matches!(
        base.mode,
        Mode::Integer
            | Mode::Double
            | Mode::Logical
            | Mode::Character
            | Mode::Complex
            | Mode::Raw
            | Mode::List
    ) {
        return None;
    }
    let length = match index.mode {
        Mode::Character => index.length,
        Mode::Integer | Mode::Double if positive_numeric_index(expression) => index.length,
        Mode::Integer | Mode::Double => {
            literal_negative_exclusion_length(base.length, expression).unwrap_or(Length::Unknown)
        }
        _ => Length::Unknown,
    };
    // Subsetting changes which fields exist; the source schema no longer applies.
    Some(RType {
        length,
        columns: None,
        ..base.clone()
    })
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

/// Whether literals, their `c(...)` concatenation, or a colon range are all
/// positive. Identifiers retain no sign information.
fn positive_numeric_index(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(index, _) => *index > 0,
        Expr::Double(index, _) => index.is_finite() && *index > 0.0 && index.fract() == 0.0,
        Expr::BinOp {
            op: BinOpKind::Colon,
            lhs,
            rhs,
            ..
        } => positive_numeric_index(lhs) && positive_numeric_index(rhs),
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
        // Without an exact count the remaining length cannot be pinned:
        // a nonempty value may hold exactly one element, so excluding
        // one position can empty it.
        Length::Unknown | Length::Nonempty => return None,
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
        Expr::Unknown(s) | Expr::Missing(s) => *s,
    }
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
/// both when a member gains a stub `eval` entry AND when a member ships
/// a stub without `eval` fields — an absent `eval` block is the stub's
/// declaration of ordinary eager evaluation, not a blank to fill from
/// this list.
///
/// A member must actually be NSE. rlang's `sym`, `abort`, `inform`,
/// `new_formula`, and `new_quosure` were removed for evaluating their
/// arguments eagerly (verified in R: `rlang::sym(undefined_name)` errors
/// with "object not found"), so suppressing RY010 inside them hid real
/// undefined-name bugs.
///
/// Stub coverage is genuinely absent for every member (issue #41):
///   * base: `makeActiveBinding` has no stub.
///   * rlang: `defuse` and `tidyeval_data` are unexported and ship no
///     stub.
///   * data.table ships no stubs.
///   * tidyselect's stub does not declare `peek_vars`. `all_vars` is
///     not here: dplyr — the package it is called through — declares
///     `expr: data_mask` for it.
pub(crate) const NSE_SYMBOL_FNS: &[&str] = &[
    // rlang NSE
    "defuse",
    "tidyeval_data",
    // tidyselect package functions
    "peek_vars",
    // base NSE helpers
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
    let group_method = split_s3_method_name(method_name, globals)
        .is_some_and(|(generic, _)| crate::semantic_lists::is_group_generic(&generic));
    // Subset primitives supply the S3 dispatch bindings too, but do not
    // belong to a group. Require a nonempty class suffix, as for group methods.
    let subset_method = ["[", "[[", "$", "[<-", "[[<-", "$<-"]
        .iter()
        .any(|generic| {
            method_name
                .strip_prefix(generic)
                .and_then(|suffix| suffix.strip_prefix('.'))
                .is_some_and(|class| !class.is_empty())
        });
    if group_method || subset_method {
        scope.insert(".Generic", RType::scalar(Mode::Character));
        scope.insert(".Method", RType::new(Mode::Character, Length::Unknown));
        scope.insert(".Class", RType::new(Mode::Character, Length::Unknown));
    }
    if group_method {
        scope.insert(".Group", RType::scalar(Mode::Character));
    }
}

/// Names assigned anywhere in a body, for closure-capture candidates.
/// Enters assignment values, `if`/`for`/`while` statement bodies,
/// braced-block values, and `if`-expression branches; records the names
/// bound by plain assignments, `for` iterators, function definitions,
/// and expression-position `<-`/`<<-`. Skips function bodies, control
/// tests (`if`/`while` conditions, `for` iterators), the assignment
/// target and `<-`/`<<-` left-operand subtrees (only the bound name is
/// recorded -- R does not evaluate them), and every expression form
/// except blocks, `if`, and assignment operators.
pub(crate) fn assigned_names_in_body(body: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    let _ = walk_stmts(
        body,
        Walk {
            assign_targets: false,
            assign_operands: false,
            fn_bodies: false,
            control_tests: false,
            ..Walk::ALL
        },
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Stmt(Stmt::Assign { target, .. }) => {
                    if let Expr::Ident { name, .. } = target {
                        names.insert(name.clone());
                    }
                }
                AstNode::Stmt(Stmt::For { name, .. }) => {
                    names.insert(name.clone());
                }
                AstNode::Expr(Expr::BinOp {
                    op: BinOpKind::Assign | BinOpKind::SuperAssign,
                    lhs,
                    ..
                }) => {
                    if let Expr::Ident { name, .. } = lhs.as_ref() {
                        names.insert(name.clone());
                    }
                }
                // Blocks and `if` expressions carry further statements;
                // the control_tests=false knob already prunes their
                // conditions. Every other expression form cannot
                // introduce names: calls, indexing, and literals only
                // read, and only assignment operators bind from
                // expression position.
                AstNode::Expr(Expr::Block { .. } | Expr::If { .. }) => {}
                AstNode::Expr(_) => return ControlFlow::Continue(Descend::Skip),
                AstNode::Stmt(_) => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
    names
}

/// The enclosing-scope bindings a function body may write through
/// superassignment (`x <<- v` / `v ->> x`), collected syntactically at
/// any nesting depth (issue #374).
///
/// `<<-` assigns in the nearest enclosing environment that already binds
/// the name, creating it in the global environment when none does; it
/// never writes the writing closure's own frame (verified against R:
/// `f <- function(x) { x <<- 10; x }` leaves the formal untouched and
/// rebinds the file-level `x`). The checker's flat scope tables cannot
/// name that frame statically, and whether the closure has run before a
/// given read is call-graph evidence ry does not have. The conservative
/// model is therefore the issue's stated strategy: every name targeted
/// by a write that can reach the definition scope -- a write directly
/// in the body, or one nested in a closure whose intervening frames do
/// not intercept it, a prune plain-name rebinding earns but complex
/// targets never do (see [`InterveningFormals`]) -- becomes
/// unknown-typed in the definition scope once the definition is walked.
/// Joining the write's type instead would keep the stale initial branch
/// (the `token <- NULL` before `token <<- 'EOF'`) inside the union, and
/// a union member R provably rejects as a condition (zero-length
/// `NULL`) still flags the whole union, so the RY001/RY010 family the
/// join targets would keep firing.
#[derive(Debug, Default)]
pub(crate) struct SuperassignmentWrites {
    /// Rebound names: plain identifier or string-literal targets
    /// (`token <<- value`), plus the root names of complex targets --
    /// `state$key <<- v` and `class(x) <<- v` rebind or mutate the
    /// object bound at the root, whose declared members are then
    /// unprovable.
    pub(crate) names: HashSet<String>,
    /// A target whose rebound root cannot be named (`f()$a <<- v`), so
    /// no single binding summarizes the write.
    pub(crate) opaque: bool,
}

impl SuperassignmentWrites {
    /// Install the collected writes as type updates in `scope`: every
    /// rebound name becomes unknown-typed (also silencing RY010 reads of
    /// a name that only ever materializes through `<<-`), and an opaque
    /// target discards all value facts because no single binding
    /// summarizes the write.
    pub(crate) fn apply(&self, scope: &mut Scope) {
        for name in &self.names {
            scope.insert(name.clone(), RType::unknown());
        }
        if self.opaque {
            scope.invalidate_unknown_effects();
        }
    }
}

/// The single binding a `<<-` target rebinds, as a name. Mirrors the
/// R6 member-rebinding shape in `r6_rebound_members`: a plain
/// identifier or string-literal target names itself, a subscripted
/// target rebinds its base (`state$key <<- v` writes `state`'s
/// binding), and a call-form target is a replacement-function
/// assignment rebinding its first argument (`class(x) <<- v`). `None`
/// when no single name summarizes the write (`f()$a <<- v`).
fn superassignment_root(target: &Expr) -> Option<String> {
    match target {
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name.clone()),
        Expr::Index { base, .. } => superassignment_root(base),
        Expr::Call { args, .. } => args
            .first()
            .map(|first| &first.value)
            .and_then(superassignment_root),
        _ => None,
    }
}

/// Whether a `<<-` target is a plain name: the identifier or
/// string-literal spelling itself, not a subscripted or call-form
/// target that [`superassignment_root`] reduces to a name.
///
/// The split matters for interception: a plain target rebinds one
/// binding, so an intervening formal fully absorbs the write, while a
/// complex target fetches the object through that formal and modifies
/// it (R's `*tmp*` protocol), which for a reference-typed root -- an
/// environment or R6 object shared between the formal and the
/// definition scope -- mutates the object in place, a write every
/// binding observes (probe: `env <- new.env(); outer <- function(env)
/// { inner <- function() env$key <<- TRUE }; outer(env)()` flips the
/// file-level `env$key`, while the same shape through a list root
/// writes only the formal's copy).
fn plain_superassignment_target(target: &Expr) -> bool {
    matches!(target, Expr::Ident { .. } | Expr::String(..))
}

/// The formal-parameter names bound by the function frames between a
/// nested `<<-` and the scope whose statements are being scanned.
///
/// `x <<- v` inside a closure F searches F's parent chain -- never F's
/// own frame -- so a PLAIN write reaches the scanned scope only when
/// no frame between the writing closure and that scope binds the name.
/// In R, `outer <- function(x) { inner <- function() x <<- TRUE }`
/// rebinds `outer`'s formal when `inner` runs; the file-level `x` is
/// untouched (an enclosing local `<-` intercepts the same way, a corner
/// this tracker deliberately leaves conservative -- see below). A
/// frame's formals therefore exclude a plain-name write from the
/// collector's results; the writing frame's own formals never do,
/// because `<<-` skips that frame entirely.
///
/// Complex targets (subscripted `env$key <<- v`, call-form `class(x)
/// <<- v`) are never excluded: R evaluates them by fetching the root
/// object through the intercepting binding and modifying it, so when
/// the root is reference-typed and shared with the definition scope
/// the mutation happens in place and the definition scope observes it
/// (verified in R: `outer <- function(e2) { inner3 <- function()
/// class(e2) <<- "foo" }; outer(e2)()` retags the file-level
/// environment). Recording those roots unconditionally errs toward
/// silence, the same tradeoff as the locals corner below.
///
/// The walker reports only the number of function bodies entered, so
/// the tracker keys each literal's formals by the walk depth at which
/// the literal itself appears. Pre-order DFS keeps that sound without
/// post-order pops: a node at depth `k` sits in the body of the most
/// recent literal visited at depth `k - 1`, and that literal's own
/// ancestors are the most recent literals at every shallower depth. A
/// sibling literal overwrites its depth's entry before the only subtree
/// that could read it (its own body) is walked.
///
/// Only formals are tracked, not locals assigned in the intermediate
/// frames. R intercepts through those locals too (`outer <- function()
/// { y <- 1; inner <- function() y <<- TRUE }` rebinds `outer`'s local),
/// but treating every plain assignment as a potential interception
/// would suppress the same diagnostics the collector exists to keep
/// alive; the corner errs toward recording the write, which silences at
/// most the one outer binding.
#[derive(Debug, Default)]
struct InterveningFormals {
    /// `frames[d]`: formals of the most recent function literal visited
    /// at walk depth `d`; that literal's body is the depth-`d + 1`
    /// subtree.
    frames: Vec<HashSet<String>>,
    /// Formals of the function whose body starts a
    /// [`superassignment_writes`] walk: its frame is the first one a
    /// depth-1 write searches before the scanned scope. The
    /// expression-position twin starts inside a `local({...})`-style
    /// block that binds nothing initially, so it keeps this empty.
    root: HashSet<String>,
}

impl InterveningFormals {
    /// Formals of the function whose body the statement walk starts
    /// from (its literal is outside the walked slice).
    fn with_root(params: &[Param]) -> Self {
        InterveningFormals {
            frames: Vec::new(),
            root: params.iter().map(|p| p.name.clone()).collect(),
        }
    }

    /// Record that the literal visited at `depth` binds `params`.
    fn enter(&mut self, depth: usize, params: &[Param]) {
        if self.frames.len() <= depth {
            self.frames.resize(depth + 1, HashSet::new());
        }
        self.frames[depth] = params.iter().map(|p| p.name.clone()).collect();
    }

    /// Whether a `<<-` at `depth` targeting `name` is intercepted by a
    /// frame between the writing frame and the scanned scope. The
    /// writing frame is the literal entered at `depth - 1` (the scanned
    /// statements themselves at `depth` 0), and `<<-` skips it; every
    /// frame after it on the chain to the scanned scope -- the walked
    /// body's own function for a statement walk, then each intervening
    /// literal's frame -- intercepts the write in R.
    fn intercepts(&self, depth: usize, name: &str) -> bool {
        (depth > 0 && self.root.contains(name))
            || self.frames[..depth.saturating_sub(1)]
                .iter()
                .any(|frame| frame.contains(name))
    }

    /// One pre-order visit. Function literals contribute their formals
    /// to the frame stack; a superassignment contributes its target
    /// unless an intervening frame intercepts the name.
    fn visit(
        &mut self,
        node: AstNode<'_>,
        depth: usize,
        writes: &mut SuperassignmentWrites,
    ) -> Descend {
        match node {
            AstNode::Expr(Expr::Function { params, .. })
            | AstNode::Stmt(Stmt::FunctionDef { params, .. }) => self.enter(depth, params),
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::SuperAssign,
                lhs,
                ..
            }) => match superassignment_root(lhs) {
                // A complex target mutates the object fetched through
                // any intercepting binding, so an in-place write to a
                // shared reference-typed root still reaches the scanned
                // scope; the root is recorded regardless of nesting.
                Some(name) if !plain_superassignment_target(lhs) => {
                    writes.names.insert(name);
                }
                Some(name) if !self.intercepts(depth, &name) => {
                    writes.names.insert(name);
                }
                // An intercepted plain rebind lands in the intervening
                // frame's own binding; the scanned scope never sees it.
                Some(_) => {}
                // A root that cannot be named could hit any binding, so
                // the write stays opaque regardless of nesting.
                None => writes.opaque = true,
            },
            _ => {}
        }
        Descend::Into
    }
}

/// Collect every superassignment target in `body` that can reach the
/// scope where the function (`params`/`body`) is defined, entering
/// nested function bodies, control-flow tests, and block values. Both
/// the statement form (`x <<- v` lowers to `Stmt::Assign` carrying a
/// `SuperAssign` marker around the value) and expression position
/// (`y <- (x <<- v)`) present the marker as an `Expr::BinOp`.
///
/// Writes nested in closures defined inside `body` are included only
/// when no intervening frame binds the name as a formal -- a rule that
/// prunes plain-name rebinding only; complex-target roots are always
/// included ([`InterveningFormals`]).
pub(crate) fn superassignment_writes(params: &[Param], body: &[Stmt]) -> SuperassignmentWrites {
    let mut writes = SuperassignmentWrites::default();
    let mut formals = InterveningFormals::with_root(params);
    let _ = walk_stmts(
        body,
        Walk::ALL,
        |node: AstNode<'_>, depth: usize| -> ControlFlow<(), Descend> {
            ControlFlow::Continue(formals.visit(node, depth, &mut writes))
        },
    );
    writes
}

/// The expression-position twin of [`superassignment_writes`], for
/// argument-supplied code blocks (`local({...})`, testthat blocks)
/// whose `<<-` writes -- nested closure bodies included, because those
/// closures outlive the block -- reach the caller's frame. The block
/// evaluates in a fresh child environment that binds nothing, so a
/// depth-0 write always climbs to the caller; deeper writes are pruned
/// by the same intervening-formal rule.
pub(crate) fn superassignment_writes_in_expr(expr: &Expr) -> SuperassignmentWrites {
    let mut writes = SuperassignmentWrites::default();
    let mut formals = InterveningFormals::default();
    let _ = walk_expr(
        expr,
        Walk::ALL,
        |node: AstNode<'_>, depth: usize| -> ControlFlow<(), Descend> {
            ControlFlow::Continue(formals.visit(node, depth, &mut writes))
        },
    );
    writes
}

/// Pins the traversal shape of [`superassignment_writes`]: statement and
/// expression positions both present the `SuperAssign` marker, nested
/// closure bodies contribute their targets (a closure defined inside
/// the body writes through the body's frame when it later runs) unless
/// an intervening frame's formal intercepts the write -- a prune only
/// plain-name targets earn -- and complex targets resolve to their
/// root name.
#[cfg(test)]
mod superassignment_writes_tests {
    use super::*;
    use std::collections::HashSet;

    fn writes_for(params_src: &str, body_src: &str) -> SuperassignmentWrites {
        let src = format!("outer <- function({params_src}) {{\n{body_src}\n}}\n");
        let file = crate::tests::parse_file("superassign_test.R", &src);
        let [
            Stmt::Assign {
                value: Expr::Function { params, body, .. },
                ..
            },
        ] = file.stmts.as_slice()
        else {
            panic!("test source must be a single `outer <- function()` assignment");
        };
        superassignment_writes(params, body)
    }

    fn names_in(body_src: &str) -> SuperassignmentWrites {
        writes_for("", body_src)
    }

    fn assert_names(body_src: &str, expected: &[&str]) {
        let writes = names_in(body_src);
        let expected: HashSet<String> = expected.iter().map(|name| name.to_string()).collect();
        assert_eq!(writes.names, expected, "names from body `{body_src}`");
        assert!(!writes.opaque, "no opaque target expected for `{body_src}`");
    }

    /// Like [`assert_names`], but the walked function itself binds
    /// `params_src` as formals: its frame is the first one a depth-1
    /// write searches before the definition scope.
    fn assert_formal_names(params_src: &str, body_src: &str, expected: &[&str]) {
        let writes = writes_for(params_src, body_src);
        let expected: HashSet<String> = expected.iter().map(|name| name.to_string()).collect();
        assert_eq!(
            writes.names, expected,
            "names from `function({params_src})` body `{body_src}`"
        );
        assert!(
            !writes.opaque,
            "no opaque target expected for `function({params_src})` body `{body_src}`"
        );
    }

    /// The issue's corpus shape: a nested closure mutates the enclosing
    /// binding only through `<<-`.
    #[test]
    fn records_targets_inside_nested_closure_bodies() {
        assert_names(
            "token <- NULL\nread <- function() token <<- 'EOF'\nwhile (token) break",
            &["token"],
        );
    }

    /// Both the statement form and expression position carry the marker,
    /// and the right-to-left spelling (`v ->> x`) lowers to the same
    /// wrapper.
    #[test]
    fn records_statement_expression_and_arrow_forms() {
        assert_names("x <<- 1", &["x"]);
        assert_names("y <- (x <<- 1)", &["x"]);
        assert_names("1 ->> x", &["x"]);
    }

    /// A complex target rebinds its root; a call-form target rebinds
    /// its first argument; a root that cannot be named makes the write
    /// opaque.
    #[test]
    fn resolves_complex_targets_to_their_root() {
        assert_names("state$key <<- 1", &["state"]);
        assert_names("class(x) <<- 1", &["x"]);
        let opaque = names_in("make()$key <<- 1");
        assert!(opaque.opaque, "call-rooted target must be opaque");
    }

    /// Negative control: plain assignment inside nested bodies targets
    /// nothing outside the writing frame.
    #[test]
    fn ignores_plain_assignment_in_nested_bodies() {
        assert_names("inner <- function() { local_x <- 1 }", &[]);
    }

    /// A nested `<<-` whose name a frame between the writing closure and
    /// the definition scope binds as a formal never reaches the
    /// definition scope: the write lands in that frame (verified against
    /// R: `outer <- function(x) { inner <- function() x <<- TRUE };
    /// outer(NULL)()` rebinds `outer`'s formal and leaves the file-level
    /// `x` untouched), so the definition-scope binding keeps its type.
    #[test]
    fn excludes_targets_intercepted_by_intervening_formals() {
        // The walked function's own formal intercepts a depth-1 write.
        assert_formal_names("x", "inner <- function() x <<- TRUE", &[]);
        // A middle literal's formal intercepts a depth-2 write while the
        // walked function binds nothing.
        assert_names("mid <- function(x) { inner <- function() x <<- TRUE }", &[]);
        // An unrelated formal of the walked function intercepts nothing.
        assert_formal_names("y", "inner <- function() x <<- TRUE", &["x"]);
        // The writing literal's own formal does not intercept: `<<-`
        // skips the writing frame, so the write still targets the
        // definition scope (probe: `inner2 <- function(x) x <<- TRUE`
        // rebinds the file-level `x`).
        assert_names("inner <- function(x) x <<- TRUE", &["x"]);
        // A sibling literal's formal must not leak into a later write:
        // `a` and `b` are side by side, not nested.
        assert_names("a <- function(s) NULL\nb <- function() s <<- TRUE", &["s"]);
    }

    /// A complex target bypasses the intervening-formal rule: R's
    /// complex superassignment fetches the root through the intercepting
    /// formal and modifies it, so a reference-typed root shared with the
    /// definition scope is mutated in place (verified against R:
    /// `outer <- function(env) { inner <- function() env$key <<- TRUE
    /// }; outer(env)()` flips the file-level `env$key`, and the
    /// call-form `class(e2) <<- "foo"` through a formal retags the
    /// file-level environment). The root stays recorded even when a
    /// value-typed root (a list) would in fact only write the formal's
    /// copy -- the conservative side of the split.
    #[test]
    fn complex_targets_bypass_the_interception_rule() {
        assert_formal_names(
            "state",
            "inner <- function() state$key <<- TRUE",
            &["state"],
        );
        assert_formal_names(
            "other",
            "inner <- function() state$key <<- TRUE",
            &["state"],
        );
        assert_formal_names("e2", "inner <- function() class(e2) <<- 'foo'", &["e2"]);
    }

    /// The expression-position twin collects from the same shapes when
    /// the code arrives as one expression (a `local({...})` argument).
    #[test]
    fn collects_from_expression_position() {
        let src = "json <- local({\n  token <- NULL\n  read <- function() token <<- 'EOF'\n})\n";
        let file = crate::tests::parse_file("superassign_expr_test.R", src);
        let [Stmt::Assign { value, .. }] = file.stmts.as_slice() else {
            panic!("test source must be a single assignment");
        };
        let Expr::Call { args, .. } = value else {
            panic!("test source must assign a call");
        };
        let writes = superassignment_writes_in_expr(&args[0].value);
        let expected: HashSet<String> = ["token"].iter().map(|s| s.to_string()).collect();
        assert_eq!(writes.names, expected);
        assert!(!writes.opaque);
    }

    /// The expression-position twin prunes intercepted writes too: the
    /// block itself binds nothing, but a closure defined inside it whose
    /// formal shadows the target keeps the write in that closure's
    /// parent chain inside the block, never the caller's frame.
    #[test]
    fn expression_position_prunes_intercepted_writes() {
        let src = "json <- local({\n  outer <- function(token) {\n    inner <- function() token <<- 'EOF'\n  }\n})\n";
        let file = crate::tests::parse_file("superassign_expr_intercept_test.R", src);
        let [Stmt::Assign { value, .. }] = file.stmts.as_slice() else {
            panic!("test source must be a single assignment");
        };
        let Expr::Call { args, .. } = value else {
            panic!("test source must assign a call");
        };
        let writes = superassignment_writes_in_expr(&args[0].value);
        assert!(writes.names.is_empty(), "{:?}", writes.names);
        assert!(!writes.opaque);
    }
}

/// Pins the traversal shape of [`assigned_names_in_body`] to the
/// hand-rolled walker it replaced: names bound inside braced-block
/// values and `if`-expression branches are locals of the enclosing
/// body (closure-capture and loop-carried-binding candidates), while
/// control tests and unevaluated assignment targets stay pruned.
#[cfg(test)]
mod assigned_names_in_body_tests {
    use super::*;
    use std::collections::HashSet;

    /// The collection runs on a function body (its callers extract the
    /// body from the literal first), so wrap the test source in one.
    fn assigned(body_src: &str) -> HashSet<String> {
        let src = format!("f <- function() {{\n{body_src}\n}}\n");
        let file = crate::tests::parse_file("assigned_names_test.R", &src);
        let [
            Stmt::Assign {
                value: Expr::Function { body, .. },
                ..
            },
        ] = file.stmts.as_slice()
        else {
            panic!("test source must be a single `f <- function()` assignment");
        };
        assigned_names_in_body(body)
    }

    fn assert_exact(body_src: &str, expected: &[&str]) {
        let found = assigned(body_src);
        let expected: HashSet<String> = expected.iter().map(|name| name.to_string()).collect();
        assert_eq!(found, expected, "names from body `{body_src}`");
    }

    /// A braced-block value carries statements, so `x` is assigned in
    /// the enclosing body. A wildcard `Expr(_) => Skip` callback arm
    /// pruned it -- the review blocker this pins.
    #[test]
    fn records_names_assigned_inside_braced_block_values() {
        assert_exact("out <- { x <- 1; out }", &["out", "x"]);
    }

    /// `if` in expression position evaluates both branches in the
    /// current environment, so bindings in either branch are locals.
    #[test]
    fn records_names_assigned_inside_if_expression_branches() {
        assert_exact("res <- if (c) a else { b <- 1 }", &["res", "b"]);
    }

    /// Negative controls: the `for` iterator is a control test and the
    /// `if`-expression condition is not walked, so assignments nested
    /// there are not recorded even though R evaluates the test.
    #[test]
    fn does_not_record_control_test_assignments() {
        assert_exact("for (i in g(a <- 1)) print(i)", &["i"]);
        assert_exact("res <- if (mk(w <- 1)) a else b", &["res"]);
    }

    /// Names bound through `<-`/`<<-` in expression position are
    /// recorded, but the left operand subtree is not walked (only the
    /// bound identifier is recorded, matching R's unevaluated target).
    #[test]
    fn records_expression_position_assignment_names_without_walking_lhs() {
        assert_exact("z <- (y <- f(x <- 1))", &["z", "y"]);
    }
}

#[cfg(test)]
mod operator_generic_tests {
    use super::is_operator_generic;

    /// The negative samples pin `is_operator_generic` to the Arith +
    /// Compare members of `semantic_lists::OPERATORS` (which the
    /// predicate reads directly, so the positive direction is a
    /// containment check against itself): Logic, assignment,
    /// sequence, and access operators are operator symbols for RY010
    /// suppression but never operator generics, `%in%` is a
    /// function-backed infix operator outside both dispatch groups,
    /// and a full method name like `+.foo` is split before this
    /// predicate runs. Reinstating a separate hardcoded symbol set
    /// here fails this test.
    #[test]
    fn non_dispatch_operators_are_not_operator_generics() {
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
