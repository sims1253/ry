use super::*;

pub(crate) fn equality_list_leaf_type(op: BinOpKind, value: &RType) -> Option<RType> {
    let _ = op;
    if !matches!(value.mode, Mode::List) {
        return None;
    }
    let Some(schema) = value.columns.as_ref() else {
        return Some(RType::new(Mode::Opaque, value.length));
    };
    if !schema.complete {
        return Some(RType::new(Mode::Opaque, value.length));
    }
    let all_atomic = schema.columns.iter().all(|(_, leaf)| {
        matches!(
            leaf.mode,
            Mode::Logical
                | Mode::Integer
                | Mode::Double
                | Mode::Complex
                | Mode::Character
                | Mode::Raw
                | Mode::Null
        )
    });
    if !all_atomic {
        None
    } else if let Some(leaf) = schema.homogeneous_element_type() {
        Some(RType::new(leaf.mode, value.length))
    } else {
        Some(RType::new(Mode::Opaque, value.length))
    }
}

/// R's foreign-function-interface primitives. Their first argument is a
/// native routine entry-point symbol (a bare identifier or backtick
/// name), not a variable reference, so RY010 must not fire on it.
pub(crate) fn is_ffi_primitive(name: &str) -> bool {
    matches!(
        name,
        ".Call" | ".C" | ".Fortran" | ".External" | ".External2" | ".Internal"
    )
}

/// Whether a purrr typed-map's callback return `mode` can coerce into
/// the target `target` mode without a lossy or surprising conversion.
/// Numeric modes (double/int/logical) coerce among themselves harmlessly;
/// a character or list return into a numeric (or vice-versa) target is
/// the real footgun RY080 targets. Opaque/unknown/union/null returns are
/// assumed compatible (no evidence of a mismatch).
pub(crate) fn modes_compatible(mode: &Mode, target: &Mode) -> bool {
    if matches!(mode, Mode::Opaque | Mode::Union | Mode::Null) {
        return true;
    }
    fn is_numeric(m: &Mode) -> bool {
        matches!(m, Mode::Double | Mode::Integer | Mode::Logical)
    }
    match target {
        Mode::Double | Mode::Integer | Mode::Logical => is_numeric(mode),
        Mode::Character => matches!(mode, Mode::Character),
        _ => true,
    }
}

/// Return the R source symbol for a binary operator, for use in
/// diagnostic messages. Returns `?` for unknown ops.
pub(crate) fn op_symbol(op: BinOpKind) -> &'static str {
    match op {
        BinOpKind::Add => "+",
        BinOpKind::Sub => "-",
        BinOpKind::Mul => "*",
        BinOpKind::Div => "/",
        BinOpKind::Pow => "^",
        BinOpKind::Mod => "%%",
        BinOpKind::IDiv => "%/%",
        BinOpKind::Colon => ":",
        BinOpKind::Lt => "<",
        BinOpKind::Le => "<=",
        BinOpKind::Gt => ">",
        BinOpKind::Ge => ">=",
        BinOpKind::Eq => "==",
        BinOpKind::Ne => "!=",
        BinOpKind::And => "&",
        BinOpKind::AndAnd => "&&",
        BinOpKind::Or => "|",
        BinOpKind::OrOr => "||",
        BinOpKind::In => "%in%",
        BinOpKind::Assign => "<-",
        BinOpKind::SuperAssign => "<<-",
        BinOpKind::PipeForward => "%>%",
        BinOpKind::PipeTee => "%T>%",
        BinOpKind::PipeAssign => "%<>%",
    }
}

/// True if `e` is the magrittr `.` data pronoun. Unlike
/// [`is_pipe_placeholder`], this excludes base-R's `_` placeholder,
/// which has no data-pronoun role: `x %>% _$col` is not valid R.
/// Used by `infer_pipe` to detect nested access forms like
/// `x %>% .$col`, `x %>% .[i]`, and `x %>% .[[i]]`, where the `.` at
/// the base of the index refers to the piped LHS value.
pub(crate) fn is_dot_pronoun(e: &Expr) -> bool {
    matches!(e, Expr::Ident { name, .. } if name == ".")
}

/// A type refinement extracted from an `if` condition. Represents the
/// information we can glean from a type predicate call like
/// `is.numeric(x)` or `is.null(x)`.
///
/// `Narrowing::Positive` means "in the `then` branch, `var` satisfies the
/// predicate". `Negative` is its negated counterpart: the `else` branch
/// satisfies the predicate, while the `then` branch may be narrowed away
/// from it when that complement is representable.
#[derive(Debug, Clone)]
pub(crate) enum Narrowing {
    /// No refinement could be extracted from the condition.
    None,
    /// `var` is narrowed to `target` in the positive (then) branch.
    /// `target` is a full RType: a scalar mode for single-mode
    /// predicates (`is.double`, `is.integer`, ...), or a union for
    /// group predicates (`is.numeric` -> union[integer, double]). This
    /// replaces the old `Mode`-only form, which could not distinguish
    /// `is.numeric` (a group) from `is.double` (a single mode) and so
    /// rewrote a known Integer to Double.
    Positive { var: String, target: RType },
    /// `var` satisfies `target` in the `else` branch of `!predicate(var)`.
    Negative { var: String, target: RType },
    /// An `||` guard whose false path proves a predicate. It deliberately
    /// has no then-branch refinement: either operand may have made the
    /// condition true.
    Else { var: String, target: RType },
    /// A zero-length guard (`!length(x)` or `length(x) == 0`) whose false
    /// path proves only that `x` is non-NULL.  This is deliberately weaker
    /// than claiming anything about its storage mode or non-emptiness.
    NonNullElse { var: String },
    /// A rejecting `||` chain containing `length(x) != 1`. Its false path
    /// proves that `x` has length one. A negated type predicate over the same
    /// variable (for example `!is.numeric(x)`) may additionally prove mode.
    ScalarElse { var: String, target: Option<RType> },
}

/// Extract a type narrowing from an `if` condition expression.
/// Recognizes:
///   * `is.numeric(x)` / `is.double(x)` / `is.integer(x)` /
///     `is.character(x)` / `is.logical(x)` / `is.complex(x)` /
///     `is.list(x)` / `is.function(x)` / `is.null(x)`
///   * negated forms of all the predicates above
///
pub(crate) fn extract_type_narrowing(cond: &Expr) -> Narrowing {
    match cond {
        Expr::Call { func, args, .. } => {
            let Expr::Ident { name, .. } = func.as_ref() else {
                return Narrowing::None;
            };
            let target = if name == "inherits" {
                args.get(1).and_then(|arg| match &arg.value {
                    Expr::String(class, _) if !class.is_empty() => {
                        Some(RType::unknown().with_class(ClassVector::single(class)))
                    }
                    _ => None,
                })
            } else {
                predicate_target(name).or_else(|| s3_predicate_target(name))
            };
            let Some(target) = target else {
                return Narrowing::None;
            };
            let Some(var) = args.first().and_then(|a| match &a.value {
                Expr::Ident { name, .. } => Some(name.clone()),
                _ => None,
            }) else {
                return Narrowing::None;
            };
            // `is.null(x)` (non-negated): fall through to Positive with
            // target = NULL. The Positive arm narrows `var` to NULL in the
            // then branch and narrows it AWAY from NULL in the else branch
            // (the case the plan calls out: `if (is.null(x)) ... else x()`).
            Narrowing::Positive { var, target }
        }
        Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } => {
            if let Some(var) = length_guard_var(expr) {
                return Narrowing::NonNullElse { var };
            }
            let Expr::Call { func, args, .. } = expr.as_ref() else {
                return Narrowing::None;
            };
            let Expr::Ident { name, .. } = func.as_ref() else {
                return Narrowing::None;
            };
            let Some(var) = args.first().and_then(|a| match &a.value {
                Expr::Ident { name, .. } => Some(name.clone()),
                _ => None,
            }) else {
                return Narrowing::None;
            };
            let target = if name == "inherits" {
                args.get(1).and_then(|arg| match &arg.value {
                    Expr::String(class, _) if !class.is_empty() => {
                        Some(RType::unknown().with_class(ClassVector::single(class)))
                    }
                    _ => None,
                })
            } else {
                predicate_target(name).or_else(|| s3_predicate_target(name))
            };
            let Some(target) = target else {
                return Narrowing::None;
            };
            Narrowing::Negative { var, target }
        }
        Expr::BinOp {
            op: BinOpKind::Eq,
            lhs,
            rhs,
            ..
        } if is_zero_literal(rhs) => {
            if let Some(var) = length_guard_var(lhs) {
                Narrowing::NonNullElse { var }
            } else {
                Narrowing::None
            }
        }
        Expr::BinOp {
            op: BinOpKind::OrOr,
            lhs,
            rhs,
            ..
        } => {
            if let Some((var, target)) = scalar_false_path_fact(cond) {
                return Narrowing::ScalarElse { var, target };
            }
            // The false path through `a || b` reaches the continuation only
            // when both operands are false. Keep this intentionally strict:
            // a null guard may contribute its non-null fact only when the
            // other operand is also a predicate over the same variable.
            let Narrowing::Positive { var, target } = extract_type_narrowing(lhs) else {
                return Narrowing::None;
            };
            if target.mode != Mode::Null || predicate_var(rhs).as_deref() != Some(&var) {
                return Narrowing::None;
            }
            Narrowing::Else { var, target }
        }
        Expr::BinOp {
            op: BinOpKind::And | BinOpKind::AndAnd,
            lhs,
            rhs,
            ..
        } => {
            // A true conjunction proves each conjunct.  In particular,
            // `if (ready & !is.null(x))` makes `x` non-null in the body;
            // retaining the NULL default there fabricates length-zero
            // comparisons such as `x %in% c("a", "b")`.
            for operand in [lhs.as_ref(), rhs.as_ref()] {
                if let Narrowing::Negative { var, target } = extract_type_narrowing(operand)
                    && target.mode == Mode::Null
                {
                    return Narrowing::Negative { var, target };
                }
            }
            Narrowing::None
        }
        _ => Narrowing::None,
    }
}

/// Fact established when a rejecting `||` chain is false. R's short-circuit
/// semantics guarantee every operand was false, so `length(x) != 1` proves
/// length one in the continuation. A false `!is.*(x)` operand independently
/// establishes its positive type predicate.
fn scalar_false_path_fact(expr: &Expr) -> Option<(String, Option<RType>)> {
    fn visit(expr: &Expr, leaves: &mut Vec<Expr>) {
        if let Expr::BinOp {
            op: BinOpKind::OrOr,
            lhs,
            rhs,
            ..
        } = expr
        {
            visit(lhs, leaves);
            visit(rhs, leaves);
        } else {
            leaves.push(expr.clone());
        }
    }

    fn length_not_one_var(expr: &Expr) -> Option<String> {
        let Expr::BinOp {
            op: BinOpKind::Ne,
            lhs,
            rhs,
            ..
        } = expr
        else {
            return None;
        };
        if is_one_literal(rhs) {
            length_guard_var(lhs)
        } else if is_one_literal(lhs) {
            length_guard_var(rhs)
        } else {
            None
        }
    }

    fn false_path_target(expr: &Expr, var: &str) -> Option<RType> {
        let Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } = expr
        else {
            return None;
        };
        let Narrowing::Positive {
            var: predicate_var,
            target,
        } = extract_type_narrowing(expr)
        else {
            return None;
        };
        (predicate_var == var && target.mode != Mode::Null).then_some(target)
    }

    let mut leaves = Vec::new();
    visit(expr, &mut leaves);
    let var = leaves.iter().find_map(length_not_one_var)?;
    let target = leaves.iter().find_map(|leaf| false_path_target(leaf, &var));
    Some((var, target))
}

fn length_guard_var(expr: &Expr) -> Option<String> {
    let Expr::Call { func, args, .. } = expr else {
        return None;
    };
    if !matches!(func.as_ref(), Expr::Ident { name, .. } if name == "length") {
        return None;
    }
    match &args.first()?.value {
        Expr::Ident { name, .. } => Some(name.clone()),
        _ => None,
    }
}

fn is_zero_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Integer(0, _)) || matches!(expr, Expr::Double(value, _) if *value == 0.0)
}

fn is_one_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Integer(1, _)) || matches!(expr, Expr::Double(value, _) if *value == 1.0)
}

/// Return the variable inspected by a simple predicate. `is.na` is included
/// here solely to recognize common compound guards such as
/// `is.null(x) || is.na(x)`; it is not itself a type refinement.
fn predicate_var(expr: &Expr) -> Option<String> {
    let Expr::Call { func, args, .. } = expr else {
        return None;
    };
    let Expr::Ident { name, .. } = func.as_ref() else {
        return None;
    };
    if name != "is.na" && predicate_target(name).is_none() && name != "inherits" {
        return None;
    }
    match &args.first()?.value {
        Expr::Ident { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Map a type predicate name to the `RType` it tests for. Group
/// predicates return a union: `is.numeric` matches integer OR double,
/// so its narrowing target is `union[integer, double]` (NOT plain
/// Double, which would rewrite a known Integer to Double).
pub(crate) fn predicate_target(name: &str) -> Option<RType> {
    let name = match name {
        // rlang's snake-case helper has the same value semantics as the base
        // predicate. Its provenance is verified by ordinary call resolution;
        // modeling the alias here preserves flow facts in importing packages.
        "is_null" => "is.null",
        name => name,
    };
    match name {
        // numeric = double or integer (a group, not a single mode).
        "is.numeric" => Some(RType::scalar(Mode::Integer).join(RType::scalar(Mode::Double))),
        "is.double" => Some(RType::scalar(Mode::Double)),
        "is.integer" => Some(RType::scalar(Mode::Integer)),
        "is.character" => Some(RType::scalar(Mode::Character)),
        "is.logical" => Some(RType::scalar(Mode::Logical)),
        "is.complex" => Some(RType::scalar(Mode::Complex)),
        "is.list" => Some(RType::scalar(Mode::List)),
        "is.function" => Some(RType::scalar(Mode::Function)),
        // Data frames are list-backed in the current type lattice. There is
        // no distinct environment mode yet, so retain its opaque storage
        // mode while recording the class evidence from the guard.
        "is.data.frame" => {
            Some(RType::scalar(Mode::List).with_class(ClassVector::single("data.frame")))
        }
        "is.environment" => Some(RType::unknown().with_class(ClassVector::single("environment"))),
        "is.null" => Some(RType::new(Mode::Null, Length::Zero)),
        "is.raw" => Some(RType::scalar(Mode::Raw)),
        _ => None,
    }
}

pub(crate) fn s3_predicate_target(name: &str) -> Option<RType> {
    let class = name.strip_prefix("is.")?;
    if class.is_empty() {
        return None;
    }
    Some(RType::unknown().with_class(ClassVector::single(class)))
}

pub(crate) fn assertion_call_target(name: &str) -> Option<RType> {
    match name {
        "assert_character_scalar" => Some(RType::scalar(Mode::Character)),
        "assert_numeric_scalar" => Some(RType::scalar(Mode::Double)),
        "assert_logical_scalar" => Some(RType::scalar(Mode::Logical)),
        "assert_integer_scalar" => Some(RType::scalar(Mode::Integer)),
        "assert_function" => Some(RType::scalar(Mode::Function)),
        _ => None,
    }
}

/// Map rlang's inlined standalone checkers to the fact they establish about
/// their first argument. These helpers are package-local rather than exported
/// from rlang, so call-site recognition also verifies their defining shape
/// when a definition is available.
pub(crate) fn standalone_check_target(name: &str) -> Option<RType> {
    match name {
        "check_bool" => Some(RType::scalar(Mode::Logical)),
        "check_string" | "check_name" => Some(RType::scalar(Mode::Character)),
        "check_number_whole" | "check_number_decimal" => {
            Some(RType::scalar(Mode::Integer).join(RType::scalar(Mode::Double)))
        }
        "check_function" | "check_closure" => Some(RType::scalar(Mode::Function)),
        "check_environment" => predicate_target("is.environment"),
        "check_symbol" | "check_arg" => {
            Some(RType::unknown().with_class(ClassVector::single("name")))
        }
        "check_call" => Some(RType::unknown().with_class(ClassVector::single("call"))),
        "check_formula" => Some(RType::unknown().with_class(ClassVector::single("formula"))),
        "check_character" => Some(RType::new(Mode::Character, Length::Unknown)),
        "check_logical" => Some(RType::new(Mode::Logical, Length::Unknown)),
        // A data frame's vector length is its column count, not a scalar
        // constraint imposed by check_data_frame().
        "check_data_frame" => Some(
            RType::new(Mode::List, Length::Unknown).with_class(ClassVector::single("data.frame")),
        ),
        _ => None,
    }
}

/// Narrow a type away from NULL: the value is known to be non-null in
/// this branch. Returns `None` when nothing changes (the type carries no
/// NULL member to remove).
///
/// - Pure `Null`: degrade to opaque (we know nothing else about it).
/// - A union containing a NULL member: rebuild the union without NULL.
///   If NULL was the only member this collapses to opaque via the empty
///   case; if exactly one non-null member remains, the union collapses
///   to that member (see `RType::union`).
/// - Anything else: unchanged (`None`).
pub(crate) fn narrow_away_from_null(t: &RType) -> Option<RType> {
    match t.mode {
        Mode::Null => Some(RType::unknown()),
        Mode::Union => {
            let members = t.members.as_ref()?;
            // Only act if at least one member is NULL.
            if !members.iter().any(|m| m.mode == Mode::Null) {
                return None;
            }
            let kept: Vec<RType> = members
                .iter()
                .filter(|m| m.mode != Mode::Null)
                .cloned()
                .collect();
            if kept.is_empty() {
                // Union was NULL-only; we only know it's non-null now.
                Some(RType::unknown())
            } else {
                Some(RType::union(Arc::from(kept)))
            }
        }
        _ => None,
    }
}

/// Apply a narrowing to produce separate scopes for the `then` and
/// `else_` branches. Returns `(then_scope, else_scope)` where each is
/// a clone of `base` with the appropriate binding updated.
///
pub(crate) fn apply_narrowing(
    base: &Scope,
    narrowing: &Narrowing,
    _has_else: bool,
) -> (Scope, Scope, HashSet<String>) {
    let (mut then_scope, mut else_scope) = (base.clone(), base.clone());
    // Names refined by narrowing (in either branch). These must NOT be
    // merged back into the parent by `merge_branch_bindings`: a refinement
    // is branch-local, and folding it into the parent would degrade a
    // precise parent type (e.g. known-NULL -> opaque) and mask later
    // errors. The parent's pre-`if` type is what holds after the `if`.
    let mut narrowed: HashSet<String> = HashSet::new();
    match narrowing {
        Narrowing::None => {}
        Narrowing::Positive { var, target } => {
            // New rule: a predicate narrows only
            // when the existing type is opaque (untyped) or a union that
            // already contains the predicate's mode. A KNOWN type is
            // never rewritten: `is.numeric(x)` on a known Integer must
            // NOT rewrite it to Double (the old coerce_rank comparison
            // did exactly that).
            if let Some(existing) = then_scope.get(var).cloned() {
                let class_narrowing = target.class.has_known_class();
                let incompatible_parameter_default =
                    then_scope.is_default_parameter(var) && !types_intersect(&existing, target);
                let should_install = incompatible_parameter_default
                    || class_narrowing
                    || match existing.mode {
                        Mode::Opaque => true,
                        // A NULL default in a function signature means "the
                        // caller may provide something else"; a positive type
                        // predicate proves the branch is in that non-default
                        // shape.
                        Mode::Null => target.mode != Mode::Null,
                        Mode::Union => {
                            // Existing union: only narrow if it contains the
                            // predicate's mode (the predicate confirms one
                            // member); otherwise leave untouched.
                            target.mode == Mode::Union
                                || existing
                                    .members
                                    .as_ref()
                                    .map(|ms| {
                                        ms.iter().any(|m| {
                                            target.mode == Mode::Union || m.mode == target.mode
                                        })
                                    })
                                    .unwrap_or(false)
                        }
                        other => {
                            // Known atomic: narrow only if it already
                            // matches the predicate (idempotent). Incompatible
                            // known modes are left untouched.
                            if target.mode == Mode::Union {
                                target
                                    .members
                                    .as_ref()
                                    .map(|ms| ms.iter().any(|m| m.mode == other))
                                    .unwrap_or(false)
                            } else {
                                other == target.mode
                            }
                        }
                    };
                if should_install
                    && (incompatible_parameter_default
                        || class_narrowing
                        || matches!(existing.mode, Mode::Opaque | Mode::Null | Mode::Union))
                {
                    then_scope.insert_narrowed(
                        var.clone(),
                        RType {
                            mode: target.mode,
                            length: existing.length,
                            ..target.clone()
                        },
                    );
                    narrowed.insert(var.clone());
                }
            }
            // For is.null, the else branch knows var is NOT null. Build this
            // scope even without an explicit `else`: a diverging guard can
            // make it the continuation scope.
            if target.mode == Mode::Null {
                if let Some(existing) = else_scope.get(var).cloned() {
                    if let Some(n) = narrow_away_from_null(&existing) {
                        else_scope.insert_narrowed(var.clone(), n);
                        narrowed.insert(var.clone());
                    }
                }
            }
        }
        Narrowing::Negative { var, target } => {
            // The true branch of a negated null predicate is non-null. Other
            // complements are not representable in the current lattice, so
            // leave them conservative and retain the useful else fact below.
            if target.mode == Mode::Null {
                if let Some(existing) = then_scope.get(var).cloned() {
                    if let Some(n) = narrow_away_from_null(&existing) {
                        then_scope.insert_narrowed(var.clone(), n);
                        narrowed.insert(var.clone());
                    }
                }
            }
            install_positive_narrowing(&mut else_scope, var, target, &mut narrowed);
        }
        Narrowing::NonNullElse { var } => {
            if let Some(existing) = else_scope.get(var).cloned() {
                if let Some(n) = narrow_away_from_null(&existing) {
                    else_scope.insert_narrowed(var.clone(), n);
                    narrowed.insert(var.clone());
                }
            }
        }
        Narrowing::Else { var, target } => {
            if let Some(existing) = else_scope.get(var).cloned()
                && let Some(n) = narrow_away_from_null(&existing)
            {
                debug_assert_eq!(target.mode, Mode::Null);
                else_scope.insert_narrowed(var.clone(), n);
                narrowed.insert(var.clone());
            }
        }
        Narrowing::ScalarElse { var, target } => {
            if let Some(existing) = else_scope.get(var).cloned() {
                // A concrete NULL local cannot satisfy length(x) == 1, so the
                // false path is unreachable. A NULL parameter default is not
                // exhaustive: callers may provide a scalar value.
                if existing.mode == Mode::Null && !else_scope.is_default_parameter(var) {
                    else_scope.unreachable = true;
                } else {
                    let mut scalar = match target {
                        Some(target) => target.clone(),
                        // A NULL default says nothing about the mode callers
                        // may supply. The length guard proves only scalarity.
                        None if existing.mode == Mode::Null => RType::unknown(),
                        None => existing,
                    };
                    scalar.length = Length::One;
                    else_scope.insert_narrowed(var.clone(), scalar);
                    narrowed.insert(var.clone());
                }
            }
        }
    }
    (then_scope, else_scope, narrowed)
}

fn install_positive_narrowing(
    scope: &mut Scope,
    var: &str,
    target: &RType,
    narrowed: &mut HashSet<String>,
) {
    let Some(existing) = scope.get(var).cloned() else {
        return;
    };
    let class_narrowing = target.class.has_known_class();
    let incompatible_parameter_default =
        scope.is_default_parameter(var) && !types_intersect(&existing, target);
    let should_install = incompatible_parameter_default
        || class_narrowing
        || matches!(existing.mode, Mode::Opaque | Mode::Null | Mode::Union);
    if should_install {
        scope.insert_narrowed(
            var.to_string(),
            RType {
                mode: target.mode,
                length: existing.length,
                ..target.clone()
            },
        );
        narrowed.insert(var.to_string());
    }
}

/// Whether two narrowing types have a representable mode intersection.
/// This deliberately ignores length and class metadata: a guard such as
/// `is.list(x)` is about storage mode, and a default value's length/class
/// says nothing about values supplied by callers.
fn types_intersect(left: &RType, right: &RType) -> bool {
    fn modes(ty: &RType) -> Vec<Mode> {
        if ty.mode == Mode::Union {
            ty.members
                .as_ref()
                .map(|members| members.iter().flat_map(modes).collect())
                .unwrap_or_default()
        } else {
            vec![ty.mode]
        }
    }
    let left = modes(left);
    let right = modes(right);
    left.iter().any(|mode| right.contains(mode))
}

/// Result of trying to read a class literal from a `class = ...`
/// argument of `structure(...)`. `Unknown` covers dynamic expressions
/// (`class = my_var`, `class = some_call()`) which we cannot resolve at
/// compile time.
pub(crate) enum ClassLiteral {
    /// A single string literal, e.g. `class = "foo"`.
    Single(String),
    /// A `c(...)` of string literals, e.g. `class = c("foo", "bar")`.
    /// Non-string elements cause the whole vector to be reported as
    /// `Unknown` (R would coerce at runtime, but we play it safe).
    Multi(Vec<String>),
    /// Anything we can't statically read.
    Unknown,
}

/// Read a class literal from the `class = ...` argument of `structure`.
/// Recognizes `"foo"`, `c("foo")`, and `c("a", "b", ...)`. Mixed-type
/// vectors, non-literal values, and anything else become `Unknown`
/// rather than producing a wrong class.
pub(crate) fn parse_class_literal(e: &Expr) -> ClassLiteral {
    match e {
        Expr::String(s, _) => ClassLiteral::Single(s.clone()),
        Expr::Call { func, args, .. } => {
            if let Expr::Ident { name, .. } = func.as_ref() {
                if name == "c" {
                    let mut names: Vec<String> = Vec::new();
                    for a in args {
                        match &a.value {
                            Expr::String(s, _) => names.push(s.clone()),
                            _ => return ClassLiteral::Unknown,
                        }
                    }
                    if names.is_empty() {
                        return ClassLiteral::Unknown;
                    }
                    return ClassLiteral::Multi(names);
                }
            }
            ClassLiteral::Unknown
        }
        _ => ClassLiteral::Unknown,
    }
}

/// Build a `ColumnSchema` from a `list(...)` / `data.frame(...)` argument
/// Match call arguments to function parameters using R's standard
/// argument matching rules. Returns a vector indexed by parameter
/// position, where each entry is the type of the argument bound to
/// that parameter (or `RType::unknown()` if no argument was provided).
///
/// Algorithm (simplified v1):
///   1. Exact name match: a named arg `x = ...` binds to the parameter
///      named `x` if one exists.
///   2. Positional fill: unmatched positional args fill remaining
///      unmatched parameters in declaration order.
///   3. `...` in the parameter list absorbs any extra args; those are
///      inaccessible by index and get `UNKNOWN`.
///
/// Partial matching (R's prefix-based arg matching) is intentionally
/// not implemented; it's rarely used in modern R code and adds
/// significant complexity.
pub(crate) fn collect_forwarded_calls_in_stmts(
    caller: &str,
    params: &[Param],
    stmts: &[Stmt],
    calls: &mut Vec<ForwardedCall>,
) {
    for statement in stmts {
        match statement {
            Stmt::Assign { target, value, .. } => {
                collect_forwarded_calls_in_expr(caller, params, target, calls);
                collect_forwarded_calls_in_expr(caller, params, value, calls);
            }
            Stmt::Expr(expr) => collect_forwarded_calls_in_expr(caller, params, expr, calls),
            Stmt::If {
                cond, then, else_, ..
            } => {
                collect_forwarded_calls_in_expr(caller, params, cond, calls);
                collect_forwarded_calls_in_stmts(caller, params, then, calls);
                if let Some(else_) = else_ {
                    collect_forwarded_calls_in_stmts(caller, params, else_, calls);
                }
            }
            Stmt::For { iter, body, .. }
            | Stmt::While {
                cond: iter, body, ..
            } => {
                collect_forwarded_calls_in_expr(caller, params, iter, calls);
                collect_forwarded_calls_in_stmts(caller, params, body, calls);
            }
            Stmt::FunctionDef { .. } => {}
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    collect_forwarded_calls_in_expr(caller, params, value, calls);
                }
            }
        }
    }
}

pub(crate) fn collect_forwarded_calls_in_expr(
    caller: &str,
    params: &[Param],
    expr: &Expr,
    calls: &mut Vec<ForwardedCall>,
) {
    match expr {
        Expr::Call { func, args, .. } => {
            if let Expr::Ident { name, .. } = func.as_ref() {
                let callee = name.rsplit_once("::").map(|(_, name)| name).unwrap_or(name);
                calls.push(ForwardedCall {
                    caller: caller.to_string(),
                    callee: callee.to_string(),
                    stub_callee: name.clone(),
                    caller_params: params.to_vec(),
                    arguments: args
                        .iter()
                        .map(|argument| {
                            let forwarded = match &argument.value {
                                Expr::Ident { name, .. } => Some(name.clone()),
                                _ => None,
                            };
                            (argument.name.clone(), forwarded)
                        })
                        .collect(),
                });
            }
            collect_forwarded_calls_in_expr(caller, params, func, calls);
            for argument in args {
                collect_forwarded_calls_in_expr(caller, params, &argument.value, calls);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_forwarded_calls_in_expr(caller, params, lhs, calls);
            collect_forwarded_calls_in_expr(caller, params, rhs, calls);
        }
        Expr::UnaryOp { expr, .. } => collect_forwarded_calls_in_expr(caller, params, expr, calls),
        Expr::Index { base, args, .. } => {
            collect_forwarded_calls_in_expr(caller, params, base, calls);
            for argument in args {
                collect_forwarded_calls_in_expr(caller, params, &argument.value, calls);
            }
        }
        Expr::Block { body, .. } => collect_forwarded_calls_in_stmts(caller, params, body, calls),
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_forwarded_calls_in_expr(caller, params, cond, calls);
            collect_forwarded_calls_in_expr(caller, params, then, calls);
            if let Some(else_) = else_ {
                collect_forwarded_calls_in_expr(caller, params, else_, calls);
            }
        }
        // A nested function has its own formals; it is collected separately
        // when it has a binding, so do not attribute its calls to this caller.
        Expr::Function { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Ident { .. }
        | Expr::Unknown(_) => {}
    }
}

impl Checker {
    /// Diagnose the narrow, provable lazy-default ordering bug where a
    /// parameter is used by an earlier top-level statement than the direct
    /// body assignment needed by its default expression.
    pub(crate) fn check_lazy_default_reachability(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        assigned: &HashSet<String>,
    ) {
        let formals: HashSet<&str> = params.iter().map(|param| param.name.as_str()).collect();

        for param in params {
            let Some(default) = &param.default else {
                continue;
            };
            let mut references = HashSet::new();
            collect_executed_identifiers(default, &mut references);

            for local in references
                .iter()
                .filter(|name| assigned.contains(name.as_str()) && !formals.contains(name.as_str()))
            {
                let Some(assign_index) = body.iter().position(|statement| {
                    matches!(statement, Stmt::Assign { target: Expr::Ident { name, .. }, .. } if name == local)
                }) else {
                    // Conditional and otherwise nested assignments are not a
                    // sufficiently precise guarantee for this rule.
                    continue;
                };

                let forced = body[..assign_index].iter().find_map(|statement| {
                    first_executed_identifier_in_stmt(statement, &param.name)
                });
                if let Some(span) = forced {
                    self.emit(
                        Severity::Warning,
                        span,
                        "RY098",
                        format!(
                            "parameter `{}` may force its default before body-local `{local}` is assigned",
                            param.name
                        ),
                    );
                    break;
                }
            }
        }
    }
}

fn first_executed_identifier_in_stmt(statement: &Stmt, wanted: &str) -> Option<Span> {
    match statement {
        Stmt::Assign { value, .. } => first_executed_identifier(value, wanted),
        Stmt::Expr(expr) => first_executed_identifier(expr, wanted),
        Stmt::If {
            cond, then, else_, ..
        } => first_executed_identifier(cond, wanted)
            .or_else(|| {
                then.iter()
                    .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
            })
            .or_else(|| {
                else_.as_ref().and_then(|statements| {
                    statements
                        .iter()
                        .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
                })
            }),
        Stmt::For { iter, body, .. } => first_executed_identifier(iter, wanted).or_else(|| {
            body.iter()
                .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
        }),
        Stmt::While { cond, body, .. } => first_executed_identifier(cond, wanted).or_else(|| {
            body.iter()
                .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
        }),
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|value| first_executed_identifier(value, wanted)),
        // Defining a closure does not evaluate its body or force captures.
        Stmt::FunctionDef { .. } => None,
    }
}

fn first_executed_identifier(expr: &Expr, wanted: &str) -> Option<Span> {
    match expr {
        Expr::Ident { name, span } => (name == wanted).then_some(*span),
        Expr::Call { func, args, .. } => first_executed_identifier(func, wanted).or_else(|| {
            args.iter()
                .find_map(|argument| first_executed_identifier(&argument.value, wanted))
        }),
        Expr::BinOp { lhs, rhs, op, .. } => {
            if matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                first_executed_identifier(rhs, wanted)
            } else {
                first_executed_identifier(lhs, wanted)
                    .or_else(|| first_executed_identifier(rhs, wanted))
            }
        }
        Expr::UnaryOp { expr, .. } => first_executed_identifier(expr, wanted),
        Expr::Index { base, args, .. } => first_executed_identifier(base, wanted).or_else(|| {
            args.iter()
                .find_map(|argument| first_executed_identifier(&argument.value, wanted))
        }),
        Expr::Block { body, .. } => body
            .iter()
            .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted)),
        Expr::If {
            cond, then, else_, ..
        } => first_executed_identifier(cond, wanted)
            .or_else(|| first_executed_identifier(then, wanted))
            .or_else(|| {
                else_
                    .as_ref()
                    .and_then(|else_| first_executed_identifier(else_, wanted))
            }),
        Expr::Function { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => None,
    }
}

fn collect_executed_identifiers(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::Ident { name, .. } => {
            names.insert(name.clone());
        }
        Expr::Call { func, args, .. } => {
            collect_executed_identifiers(func, names);
            for argument in args {
                collect_executed_identifiers(&argument.value, names);
            }
        }
        Expr::BinOp { lhs, rhs, op, .. } => {
            if !matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                collect_executed_identifiers(lhs, names);
            }
            collect_executed_identifiers(rhs, names);
        }
        Expr::UnaryOp { expr, .. } => collect_executed_identifiers(expr, names),
        Expr::Index { base, args, .. } => {
            collect_executed_identifiers(base, names);
            for argument in args {
                collect_executed_identifiers(&argument.value, names);
            }
        }
        Expr::Block { body, .. } => {
            for statement in body {
                collect_identifiers_in_stmt(statement, names);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_executed_identifiers(cond, names);
            collect_executed_identifiers(then, names);
            if let Some(else_) = else_ {
                collect_executed_identifiers(else_, names);
            }
        }
        Expr::Function { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => {}
    }
}

fn collect_identifiers_in_stmt(statement: &Stmt, names: &mut HashSet<String>) {
    match statement {
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            collect_executed_identifiers(value, names);
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            collect_executed_identifiers(cond, names);
            for statement in then {
                collect_identifiers_in_stmt(statement, names);
            }
            if let Some(else_) = else_ {
                for statement in else_ {
                    collect_identifiers_in_stmt(statement, names);
                }
            }
        }
        Stmt::For { iter, body, .. }
        | Stmt::While {
            cond: iter, body, ..
        } => {
            collect_executed_identifiers(iter, names);
            for statement in body {
                collect_identifiers_in_stmt(statement, names);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                collect_executed_identifiers(value, names);
            }
        }
        Stmt::FunctionDef { .. } => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArgumentMatch {
    /// Formal parameter index for each actual argument. `None` means the
    /// argument was unmatched (or was absorbed by `...`).
    pub(crate) param_for_arg: Vec<Option<usize>>,
    pub(crate) bound_params: Vec<bool>,
    pub(crate) unmatched_named: Vec<usize>,
    pub(crate) dots: Option<usize>,
}

/// Match R call arguments in the same three passes as `match.call`: exact
/// names, unambiguous partial names, then unnamed arguments positionally.
/// Partial and positional matching stop at `...`; exact names may still bind
/// formals declared after it.
pub(crate) fn match_arguments(param_names: &[&str], args: &[Arg]) -> ArgumentMatch {
    let dots = param_names.iter().position(|name| *name == "...");
    let partial_end = dots.unwrap_or(param_names.len());
    let mut result = ArgumentMatch {
        param_for_arg: vec![None; args.len()],
        bound_params: vec![false; param_names.len()],
        unmatched_named: Vec::new(),
        dots,
    };

    // Pass 1: exact names match every formal, including formals after `...`.
    for (argument_index, argument) in args.iter().enumerate() {
        let Some(name) = argument.name.as_deref() else {
            continue;
        };
        if let Some(parameter_index) = param_names
            .iter()
            .position(|parameter| *parameter != "..." && *parameter == name)
        {
            result.param_for_arg[argument_index] = Some(parameter_index);
            result.bound_params[parameter_index] = true;
        }
    }

    // Pass 2: only a unique prefix of a pre-dots formal is a partial match.
    for (argument_index, argument) in args.iter().enumerate() {
        if result.param_for_arg[argument_index].is_some() {
            continue;
        }
        let Some(name) = argument.name.as_deref() else {
            continue;
        };
        let mut candidates =
            param_names[..partial_end]
                .iter()
                .enumerate()
                .filter(|(index, parameter)| {
                    !result.bound_params[*index] && parameter.starts_with(name)
                });
        let first = candidates.next().map(|(index, _)| index);
        if let Some(parameter_index) = first
            && candidates.next().is_none()
        {
            result.param_for_arg[argument_index] = Some(parameter_index);
            result.bound_params[parameter_index] = true;
        }
    }

    // Pass 3: unnamed actuals fill the remaining pre-dots formals in order.
    let mut next_parameter = 0;
    for (argument_index, argument) in args.iter().enumerate() {
        if argument.name.is_some() {
            if result.param_for_arg[argument_index].is_none() {
                result.unmatched_named.push(argument_index);
            }
            continue;
        }
        while next_parameter < partial_end && result.bound_params[next_parameter] {
            next_parameter += 1;
        }
        if next_parameter < partial_end {
            result.param_for_arg[argument_index] = Some(next_parameter);
            result.bound_params[next_parameter] = true;
            next_parameter += 1;
        }
    }
    result
}

pub(crate) fn match_args_to_params(
    sig_params: &[ParamSpec],
    args: &[Arg],
    arg_types: &[RType],
) -> Vec<RType> {
    let names: Vec<&str> = sig_params.iter().map(|param| param.name.as_str()).collect();
    let bindings = match_arguments(&names, args);
    let mut matched = vec![RType::unknown(); sig_params.len()];
    for (argument_index, parameter_index) in bindings.param_for_arg.iter().enumerate() {
        if let Some(parameter_index) = parameter_index
            && let Some(argument_type) = arg_types.get(argument_index)
        {
            matched[*parameter_index] = argument_type.clone();
        }
    }
    matched
}

impl Checker {
    /// Non-firing policy for schema calls:
    /// - RY090 stays silent for `...`, successful exact/partial matches, and
    ///   legacy inference-only signatures without completeness metadata.
    /// - RY091 stays silent for every non-required or successfully bound
    ///   parameter.
    /// - RY092 stays silent without a declared type, for opaque/unknown
    ///   actuals, whenever a union has any compatible overlap, and for R's
    ///   logical/integer/double coercion family.
    pub(crate) fn check_typeshed_call_arguments(
        &mut self,
        function_name: &str,
        signature: &FunctionSig,
        args: &[Arg],
        arg_types: &[RType],
        call_span: Span,
    ) {
        let names: Vec<&str> = signature.param_names().collect();
        let bindings = match_arguments(&names, args);

        // `...` accepts every otherwise-unmatched actual argument. Without
        // it, report only named arguments; excess positionals are outside
        // this rule's deliberately narrow scope.
        let supports_unknown_argument_check = signature
            .params
            .iter()
            .any(|param| param.required || param.default.is_some() || param.type_.is_some());
        self.emit_unknown_arguments(
            function_name,
            &names,
            args,
            &bindings,
            supports_unknown_argument_check,
        );
        let required: Vec<bool> = signature
            .params
            .iter()
            .map(|param| param.required)
            .collect();
        self.emit_missing_required(function_name, &names, &required, &bindings, call_span);

        for (argument_index, parameter_index) in bindings.param_for_arg.iter().enumerate() {
            let Some(parameter_index) = parameter_index else {
                continue;
            };
            let parameter = &signature.params[*parameter_index];
            let Some(expected_json) = parameter.type_.as_ref() else {
                continue;
            };
            let expected = json_rtype_to_rtype(expected_json);
            let Some(actual) = arg_types.get(argument_index) else {
                continue;
            };
            if generic_argument_may_dispatch(function_name, actual) {
                continue;
            }
            if types_provably_incompatible(actual, &expected) {
                self.emit(
                    Severity::Error,
                    args[argument_index].span,
                    "RY092",
                    format!(
                        "argument `{}` to `{function_name}` is `{}`, expected {}",
                        parameter.name,
                        actual.mode,
                        expected_type_label(&expected)
                    ),
                );
            }
        }
    }

    pub(crate) fn check_user_call_arguments(
        &mut self,
        function_name: &str,
        function: &UserFn,
        args: &[Arg],
        call_span: Span,
    ) {
        let names: Vec<&str> = function
            .params
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        let bindings = match_arguments(&names, args);
        self.emit_unknown_arguments(function_name, &names, args, &bindings, true);
        let required: Vec<bool> = function
            .params
            .iter()
            .map(|parameter| parameter.required)
            .collect();
        self.emit_missing_required(function_name, &names, &required, &bindings, call_span);
    }

    fn emit_unknown_arguments(
        &mut self,
        function_name: &str,
        names: &[&str],
        args: &[Arg],
        bindings: &ArgumentMatch,
        enabled: bool,
    ) {
        if !enabled || bindings.dots.is_some() {
            return;
        }
        for argument_index in &bindings.unmatched_named {
            let argument = &args[*argument_index];
            let argument_name = argument.name.as_deref().unwrap_or_default();
            let suggestion = closest_parameter(argument_name, names);
            let hint = suggestion
                .map(|name| format!("; did you mean `{name}`?"))
                .unwrap_or_default();
            self.emit(
                Severity::Warning,
                argument.span,
                "RY090",
                format!("unknown argument `{argument_name}` to `{function_name}`{hint}"),
            );
        }
    }

    fn emit_missing_required(
        &mut self,
        function_name: &str,
        names: &[&str],
        required: &[bool],
        bindings: &ArgumentMatch,
        call_span: Span,
    ) {
        for (parameter_index, required) in required.iter().enumerate() {
            if *required && !bindings.bound_params[parameter_index] {
                self.emit(
                    Severity::Warning,
                    call_span,
                    "RY091",
                    format!(
                        "missing required argument `{}` in call to `{function_name}`",
                        names[parameter_index]
                    ),
                );
            }
        }
    }
}

fn closest_parameter<'a>(argument: &str, parameters: &'a [&str]) -> Option<&'a str> {
    parameters
        .iter()
        .copied()
        .filter(|parameter| *parameter != "...")
        .map(|parameter| (edit_distance(argument, parameter), parameter))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, parameter)| (*distance, *parameter))
        .map(|(_, parameter)| parameter)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(right_chars.len() + 1);
        current.push(left_index + 1);
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_char != *right_char);
            current.push(
                (current[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(substitution),
            );
        }
        previous = current;
    }
    previous[right_chars.len()]
}

pub(crate) fn types_provably_incompatible(actual: &RType, expected: &RType) -> bool {
    let Some(actual_modes) = known_modes(actual) else {
        return false;
    };
    let Some(expected_modes) = known_modes(expected) else {
        return false;
    };
    !actual_modes.iter().any(|actual_mode| {
        expected_modes
            .iter()
            .any(|expected_mode| compatible_mode_pair(*actual_mode, *expected_mode))
    })
}

/// Whether every value represented by `actual` is rejected by a standalone
/// checker accepting `expected`. Unlike ordinary argument compatibility,
/// standalone checks are exact assertions: their length and class constraints
/// are runtime preconditions, and numeric modes are not interchangeable.
pub(crate) fn standalone_check_provably_rejects(actual: &RType, expected: &RType) -> bool {
    fn members(rtype: &RType) -> Vec<&RType> {
        if rtype.mode == Mode::Union {
            rtype
                .members
                .as_deref()
                .map(|members| members.iter().collect())
                .unwrap_or_else(|| vec![rtype])
        } else {
            vec![rtype]
        }
    }

    fn lengths_overlap(actual: Length, expected: Length) -> bool {
        actual == Length::Unknown || expected == Length::Unknown || actual == expected
    }

    fn classes_overlap(actual: &RType, expected: &RType) -> bool {
        if !expected.class.has_known_class() {
            return true;
        }
        if actual.class.is_unknown() {
            return true;
        }
        expected
            .class
            .names
            .iter()
            .flatten()
            .any(|name| actual.class.contains(name))
    }

    fn shapes_overlap(actual: &RType, expected: &RType) -> bool {
        let modes_overlap = actual.mode == Mode::Opaque
            || expected.mode == Mode::Opaque
            || actual.mode == expected.mode;
        modes_overlap
            && lengths_overlap(actual.length, expected.length)
            && classes_overlap(actual, expected)
    }

    !members(actual).into_iter().any(|actual| {
        members(expected)
            .into_iter()
            .any(|expected| shapes_overlap(actual, expected))
    })
}

fn generic_argument_may_dispatch(function_name: &str, actual: &RType) -> bool {
    matches!(function_name, "round" | "mean" | "log" | "sqrt" | "exp")
        && (actual.class.has_known_class() || actual.mode == Mode::Null)
}

fn known_modes(rtype: &RType) -> Option<Vec<Mode>> {
    match rtype.mode {
        Mode::Opaque => None,
        Mode::Union => {
            let members = rtype.members.as_ref()?;
            if members.iter().any(|member| member.mode == Mode::Opaque) {
                None
            } else {
                Some(members.iter().map(|member| member.mode).collect())
            }
        }
        mode => Some(vec![mode]),
    }
}

fn compatible_mode_pair(actual: Mode, expected: Mode) -> bool {
    fn numeric(mode: Mode) -> bool {
        matches!(mode, Mode::Logical | Mode::Integer | Mode::Double)
    }
    actual == expected || (numeric(actual) && numeric(expected))
}

pub(crate) fn expected_type_label(expected: &RType) -> String {
    let Some(modes) = known_modes(expected) else {
        return "unknown".to_string();
    };
    if modes.len() >= 3
        && modes.iter().all(|mode| {
            matches!(
                mode,
                Mode::Logical | Mode::Integer | Mode::Double | Mode::Complex
            )
        })
    {
        return "numeric".to_string();
    }
    modes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" or ")
}

pub(crate) fn argument_eval_mode(
    sig: &FunctionSig,
    args: &[Arg],
    index: usize,
) -> Option<EvalMode> {
    args.get(index)?;
    let names: Vec<&str> = sig.param_names().collect();
    let bindings = match_arguments(&names, args);
    let parameter = bindings.param_for_arg[index]
        .and_then(|parameter_index| names.get(parameter_index).copied())
        .unwrap_or("...");
    sig.eval
        .get(parameter)
        .copied()
        .or_else(|| sig.eval.get("...").copied())
}

/// Locate the supplied argument named by a signature's data-mask source.
/// Formula APIs place `data` after their quoted formula, and some calls put it
/// after mask-evaluated arguments, so callers must not assume argument zero.
pub(crate) fn data_mask_source_arg(sig: &FunctionSig, args: &[Arg]) -> Option<usize> {
    let source = sig.data_mask_source.as_deref()?;
    let names: Vec<&str> = sig.param_names().collect();
    let source_parameter = names.iter().position(|name| *name == source)?;
    let bindings = match_arguments(&names, args);
    bindings
        .param_for_arg
        .iter()
        .position(|parameter| *parameter == Some(source_parameter))
}

/// Resolve the resulting mode for `c(...)`. If any argument was a union,
/// the coerce-rank ladder doesn't apply soundly, so degrade to opaque
/// rather than emitting a malformed union.
pub(crate) fn collapse_c_mode(mode: Mode, saw_union: bool) -> Mode {
    if saw_union { Mode::Opaque } else { mode }
}

/// If `e` is a literal expression (`42`, `"x"`, `TRUE`, `NULL`, `NA`),
/// return the mode that calling it would error with.
/// Non-literal callees return `None` so the caller stays silent.
pub(crate) fn literal_callee_mode(e: &Expr) -> Option<Mode> {
    match e {
        Expr::Logical(_, _) => Some(Mode::Logical),
        Expr::Integer(_, _) => Some(Mode::Integer),
        Expr::Double(_, _) => Some(Mode::Double),
        Expr::String(_, _) => Some(Mode::Character),
        Expr::Null(_) => Some(Mode::Null),
        // `NA` carries its own mode (NA, NA_real_, NA_integer_, ...).
        Expr::Na(t, _) => Some(t.mode),
        _ => None,
    }
}

/// Compute the longest known length among a slice of argument types.
/// Used by `paste` / `paste0` / `sprintf` which return a character
/// vector whose length is the longest of the input vectors (R recycles
/// shorter args to match). Returns `Length::Unknown` if any arg has an
/// unknown length.
pub(crate) fn longest_arg_length(arg_types: &[RType]) -> Length {
    let mut max: Length = Length::One;
    for t in arg_types {
        max = match (max, t.length) {
            (Length::Zero, x) | (x, Length::Zero) => x,
            (Length::One, x) | (x, Length::One) => x,
            (Length::Known(a), Length::Known(b)) => Length::Known(a.max(b)),
            _ => return Length::Unknown,
        };
    }
    max
}

/// Compute the result length of `rep(x, times)`. `x` is arg0, `times`
/// is arg1. R's `rep(x, times)` returns `x` repeated `times` times; the
/// total length is `length(x) * times`. We can only compute this when
/// both lengths are known and `times` is a single integer.
pub(crate) fn rep_length(arg_types: &[RType]) -> Length {
    let x_len = arg_types
        .first()
        .map(|t| t.length)
        .unwrap_or(Length::Unknown);
    let times_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
    match (x_len, times_type.length) {
        (Length::Known(x), Length::One) => {
            // We know the structure but not the runtime `times` value;
            // approximate as Unknown unless `times` is a length-1 value
            // (which it is). R's `rep(x, 3)` gives `length(x) * 3`, but
            // we can't know the value `3` statically. Return Unknown.
            let _ = x;
            Length::Unknown
        }
        _ => Length::Unknown,
    }
}

/// Build a `ColumnSchema` from a `list(...)` / `data.frame(...)` argument
/// list. Each named arg becomes a column keyed by its name; positional
/// args get R's auto-generated `[[i]]` names (1-indexed). Returns `None`
/// if there are no args at all (an empty list has no useful schema).
///
/// The arg-type vector and the arg list must be the same length; if they
/// differ (which shouldn't happen but we guard anyway) we zip by the
/// shorter one to avoid index panics.
pub(crate) fn build_named_schema(arg_types: &[RType], args: &[Arg]) -> Option<ColumnSchema> {
    if args.is_empty() {
        return None;
    }
    let mut positional = 0usize;
    let mut columns: Vec<(String, RType)> = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let ty = arg_types.get(i).cloned().unwrap_or(RType::unknown());
        let name = match a.name.as_deref() {
            Some(n) if !n.is_empty() => semantic_argument_name(n),
            _ => {
                // R auto-generates `[[1]]`, `[[2]], ... for unnamed list
                // elements. We count only unnamed slots (named args do
                // not consume positional indices in R's `list()`, but
                // they do in `data.frame()`; for v1 we use a simple
                // running counter over all args, which matches the
                // common case and avoids surprising schema gaps).
                positional += 1;
                format!("[[{}]]", positional)
            }
        };
        columns.push((name, ty));
    }
    Some(ColumnSchema {
        columns,
        complete: true,
        locally_constructed: false,
    })
}

/// `data.frame()` derives names for simple positional expressions from the
/// expression itself (`data.frame(y, K)` has columns `y` and `K`). Lists do
/// not: their unnamed elements retain positional placeholders. Keep the two
/// constructor rules separate so improving data-frame fidelity cannot change
/// list indexing semantics.
pub(crate) fn build_data_frame_schema(arg_types: &[RType], args: &[Arg]) -> Option<ColumnSchema> {
    let mut schema = build_named_schema(arg_types, args)?;
    for ((name, _), arg) in schema.columns.iter_mut().zip(args) {
        if arg.name.is_none() {
            let Expr::Ident { name: symbol, .. } = &arg.value else {
                // Unlike list placeholders, `[[i]]` is not a reliable
                // data-frame column name. If an expression's resulting names
                // are unknown, keep the whole schema opaque so a fabricated
                // name can never justify RY060.
                return None;
            };
            *name = symbol.clone();
        }
    }
    Some(schema)
}

pub(crate) fn semantic_argument_name(name: &str) -> String {
    if name.len() >= 2 {
        let bytes = name.as_bytes();
        let quoted = matches!(
            (bytes[0], bytes[name.len() - 1]),
            (b'"', b'"') | (b'\'', b'\'') | (b'`', b'`')
        );
        if quoted {
            return name[1..name.len() - 1].to_string();
        }
    }
    name.to_string()
}

/// Convert a typeshed `JsonRType` to the checker's `RType`. Mirrors the
/// inline conversion in `apply_sig` for `ReturnSpec::Concrete` - kept
/// here in ry-checker (not ry-typeshed) so that crate stays free of any
/// dependency on ry-core's type definitions.
///
/// Datasets with an explicit `class` field (e.g. `mtcars` with
/// `["data.frame"]`) carry the class through, interning each name into a
/// `&'static str` so the result stays `Copy`. A `columns` map (for
/// data-frame datasets) is interned into a `&'static ColumnSchema` and
/// attached via `RType::with_columns`; each column's `JsonRType` is
/// converted recursively (without re-parsing nested `columns`, which
/// would be a meaningless infinite recursion for a 1-level dataset
/// schema).
pub(crate) fn json_rtype_to_rtype(jt: &JsonRType) -> RType {
    let base = json_rtype_scalar(jt);
    if jt.columns.is_empty() {
        return base;
    }
    // Build the column schema. We recurse via a single-level helper so
    // a dataset's `columns.<col>.columns` (which is empty in practice)
    // does not trigger further nesting.
    let cols: Vec<(String, RType)> = jt
        .columns
        .iter()
        .map(|(name, child)| (name.clone(), json_rtype_to_rtype_shallow(child)))
        .collect();
    let schema = Arc::new(ColumnSchema {
        columns: cols,
        complete: true,
        locally_constructed: false,
    });
    base.with_columns(schema)
}

/// Single-level variant of `json_rtype_to_rtype` for column entries
/// inside a dataset schema. Identical to the parent function except it
/// ignores any `columns` field on the child (data-frame columns are
/// plain atomic vectors in the typeshed; nested data frames are out of
/// scope for v1).
pub(crate) fn json_rtype_to_rtype_shallow(jt: &JsonRType) -> RType {
    json_rtype_scalar(jt)
}

fn json_rtype_scalar(jt: &JsonRType) -> RType {
    let length = match JsonLength::parse(&jt.length) {
        Some(JsonLength::Known(0)) => Length::Zero,
        Some(JsonLength::Known(1)) => Length::One,
        Some(JsonLength::Known(value)) => Length::Known(value),
        _ => Length::Unknown,
    };
    if matches!(JsonMode::parse(&jt.mode), Some(JsonMode::Union)) {
        let members: Vec<RType> = jt
            .members
            .iter()
            .filter_map(|member| concrete_json_mode(member))
            .map(|mode| RType::new(mode, length))
            .collect();
        return if members.is_empty() {
            RType::unknown()
        } else {
            RType::union(Arc::from(members))
        };
    }
    let mode = match JsonMode::parse(&jt.mode) {
        Some(JsonMode::Logical) => Mode::Logical,
        Some(JsonMode::Integer) => Mode::Integer,
        Some(JsonMode::Double) => Mode::Double,
        Some(JsonMode::Character) => Mode::Character,
        Some(JsonMode::Complex) => Mode::Complex,
        Some(JsonMode::Raw) => Mode::Raw,
        Some(JsonMode::List) => Mode::List,
        Some(JsonMode::Null) => Mode::Null,
        Some(JsonMode::Function) => Mode::Function,
        Some(JsonMode::Opaque) => Mode::Opaque,
        _ => Mode::Opaque,
    };
    let class = if jt.class.is_empty() {
        ClassVector::empty()
    } else {
        let refs: Vec<&str> = jt.class.iter().map(|s| s.as_str()).collect();
        ClassVector::from_slice(&refs)
    };
    RType::new(mode, length).with_class(class)
}

fn concrete_json_mode(mode: &str) -> Option<Mode> {
    Some(match JsonMode::parse(mode)? {
        JsonMode::Logical => Mode::Logical,
        JsonMode::Integer => Mode::Integer,
        JsonMode::Double => Mode::Double,
        JsonMode::Character => Mode::Character,
        JsonMode::Complex => Mode::Complex,
        JsonMode::Raw => Mode::Raw,
        JsonMode::List => Mode::List,
        JsonMode::Null => Mode::Null,
        JsonMode::Function => Mode::Function,
        JsonMode::Opaque => Mode::Opaque,
        _ => return None,
    })
}

#[cfg(test)]
mod argument_matching_tests {
    use super::*;

    fn argument(name: Option<&str>) -> Arg {
        Arg {
            name: name.map(str::to_string),
            value: Expr::Null(Span::default()),
            span: Span::default(),
        }
    }

    #[test]
    fn exact_names_are_matched_before_positionals() {
        let args = [argument(Some("second")), argument(None)];
        let matched = match_arguments(&["first", "second"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(1), Some(0)]);
        assert_eq!(matched.bound_params, vec![true, true]);
    }

    #[test]
    fn exact_match_is_removed_before_partial_matching() {
        let args = [argument(Some("alpha")), argument(Some("al"))];
        let matched = match_arguments(&["alpha", "alpine"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0), Some(1)]);
        assert!(matched.unmatched_named.is_empty());
    }

    #[test]
    fn unique_partial_name_matches() {
        let args = [argument(Some("alp"))];
        let matched = match_arguments(&["alpha", "beta"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0)]);
        assert!(matched.unmatched_named.is_empty());
    }

    #[test]
    fn ambiguous_partial_name_stays_unmatched() {
        let args = [argument(Some("al"))];
        let matched = match_arguments(&["alpha", "alpine"], &args);
        assert_eq!(matched.param_for_arg, vec![None]);
        assert_eq!(matched.unmatched_named, vec![0]);
    }

    #[test]
    fn dots_absorb_remaining_arguments_and_stop_positionals() {
        let args = [argument(None), argument(None), argument(Some("extra"))];
        let matched = match_arguments(&["x", "...", "after"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0), None, None]);
        assert_eq!(matched.unmatched_named, vec![2]);
        assert_eq!(matched.dots, Some(1));
    }

    #[test]
    fn exact_name_after_dots_still_matches_but_partial_does_not() {
        let args = [argument(Some("after")), argument(Some("aft"))];
        let matched = match_arguments(&["x", "...", "after"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(2), None]);
    }

    #[test]
    fn opaque_union_member_keeps_type_check_silent() {
        let actual = RType::union(Arc::from(vec![
            RType::unknown(),
            RType::scalar(Mode::Character),
        ]));
        let expected = RType::scalar(Mode::Double);
        assert!(!types_provably_incompatible(&actual, &expected));
    }

    #[test]
    fn closest_parameter_is_limited_to_edit_distance_two() {
        assert_eq!(
            closest_parameter("lenght", &["length", "x"]),
            Some("length")
        );
        assert_eq!(closest_parameter("unrelated", &["length", "x"]), None);
    }
}
