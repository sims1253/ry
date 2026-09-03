use super::*;
use crate::semantic_lists::DEFUSING_CALLS;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmts};
use std::ops::ControlFlow;

pub(crate) fn equality_list_leaf_type(value: &RType) -> Option<RType> {
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

/// Wrappers that forward their first argument to an FFI primitive, so it is
/// a native routine symbol under the same convention. Unlike the primitives
/// these are ordinary R functions a user could redefine, so callers gate
/// them on `useDynLib(..., .registration = TRUE)` being declared.
///
/// `call_with_cleanup` is the cleancall wrapper vendored by purrr, cli and
/// rlang: `call_with_cleanup(map_impl, environment(), ...)`.
pub(crate) fn is_registered_ffi_wrapper(name: &str) -> bool {
    matches!(name, "call_with_cleanup")
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
    match target {
        Mode::Double | Mode::Integer | Mode::Logical => numeric_family(*mode),
        Mode::Character => matches!(mode, Mode::Character),
        _ => true,
    }
}

/// R's silent-coercion numeric family (#169): logical, integer, and
/// double interchange losslessly, so compatibility questions treat
/// them as one family. Complex is excluded: coercing it into the
/// family discards imaginary parts with a warning, and base R's
/// `is.numeric()` says no. The one deliberate exception is
/// `expected_type_label`'s "numeric" wording, which covers complex.
fn numeric_family(mode: Mode) -> bool {
    matches!(mode, Mode::Double | Mode::Integer | Mode::Logical)
}

/// Every storage mode a type may hold, unions flattened recursively.
/// Opaque participates as an ordinary mode (the permissive view used by
/// `types_intersect`); a union without a member list -- a state
/// `RType::union`'s contract forbids -- yields none.
fn modes_of(ty: &RType) -> Vec<Mode> {
    match ty.mode {
        Mode::Union => ty
            .members
            .as_ref()
            .map(|members| members.iter().flat_map(modes_of).collect())
            .unwrap_or_default(),
        mode => vec![mode],
    }
}

/// `modes_of` restricted to fully knowable sets: `None` when the type,
/// any union member at any depth, or a member-less union is opaque.
/// `None` is a proof barrier -- callers treat it as "cannot decide",
/// never as evidence of a mismatch.
fn mode_set(ty: &RType) -> Option<Vec<Mode>> {
    let modes = modes_of(ty);
    (!modes.is_empty() && !modes.contains(&Mode::Opaque)).then_some(modes)
}

/// Return the R source symbol for a binary operator, for use in
/// diagnostic messages.
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
        BinOpKind::PipeNative => "|>",
        BinOpKind::PipeTee => "%T>%",
        BinOpKind::PipeAssign => "%<>%",
    }
}

/// Whether `op` is one of R's six comparison operators.
pub(crate) fn is_comparison(op: BinOpKind) -> bool {
    matches!(
        op,
        BinOpKind::Lt
            | BinOpKind::Le
            | BinOpKind::Gt
            | BinOpKind::Ge
            | BinOpKind::Eq
            | BinOpKind::Ne
    )
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
/// The variable named by a predicate's first argument, if that argument is
/// a bare identifier.
fn first_arg_ident(args: &[Arg]) -> Option<String> {
    args.first().and_then(|a| match &a.value {
        Expr::Ident { name, .. } => Some(name.clone()),
        _ => None,
    })
}

/// The `RType` a predicate call tests for. `inherits(x, "foo")` reads its
/// class from the second argument; every other name maps through
/// `predicate_target`, then the `is.<class>` fallback.
fn predicate_call_target(name: &str, args: &[Arg]) -> Option<RType> {
    if name == "inherits" {
        args.get(1).and_then(|arg| match &arg.value {
            Expr::String(class, _) if !class.is_empty() => {
                Some(RType::unknown().with_class(ClassVector::single(class)))
            }
            _ => None,
        })
    } else {
        predicate_target(name).or_else(|| s3_predicate_target(name))
    }
}

pub(crate) fn extract_builtin_type_narrowing(cond: &Expr) -> Narrowing {
    match cond {
        Expr::Call { func, args, .. } => {
            let Expr::Ident { name, .. } = func.as_ref() else {
                return Narrowing::None;
            };
            let Some(target) = predicate_call_target(name, args) else {
                return Narrowing::None;
            };
            let Some(var) = first_arg_ident(args) else {
                return Narrowing::None;
            };
            // `is.null(x)` (non-negated): fall through to Positive with
            // target = NULL. The Positive arm narrows `var` to NULL in the
            // then branch and narrows it AWAY from NULL in the else branch
            // (the motivating case: `if (is.null(x)) ... else x()`).
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
            let Some(var) = first_arg_ident(args) else {
                return Narrowing::None;
            };
            let Some(target) = predicate_call_target(name, args) else {
                return Narrowing::None;
            };
            Narrowing::Negative { var, target }
        }
        Expr::BinOp {
            op: BinOpKind::Eq,
            lhs,
            rhs,
            ..
        } if is_literal_eq(rhs, 0.0) => {
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
            let Narrowing::Positive { var, target } = extract_builtin_type_narrowing(lhs) else {
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
                if let Narrowing::Negative { var, target } = extract_builtin_type_narrowing(operand)
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
        if is_literal_eq(rhs, 1.0) {
            length_guard_var(lhs)
        } else if is_literal_eq(lhs, 1.0) {
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
        } = extract_builtin_type_narrowing(expr)
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
    first_arg_ident(args)
}

/// Whether `expr` is the whole-number literal `value` (`0`, `1`, `1.0`),
/// for the length-guard shapes `length(x) == 0` / `length(x) != 1`.
fn is_literal_eq(expr: &Expr, value: f64) -> bool {
    match expr {
        Expr::Integer(n, _) => *n as f64 == value,
        Expr::Double(n, _) => *n == value,
        _ => false,
    }
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
    first_arg_ident(args)
}

/// Map a type predicate name to the `RType` it tests for. Group
/// predicates return a union: `is.numeric` matches integer OR double,
/// so its narrowing target is `union[integer, double]` (NOT plain
/// Double, which would rewrite a known Integer to Double).
pub(crate) fn predicate_target(name: &str) -> Option<RType> {
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

/// Narrowing targets for `assert_*_scalar` calls. This map and the
/// stub-driven assertion machinery in `infer_call` (a signature's
/// `assertion` field, e.g. rlang's `check_bool`) encode the same
/// knowledge: a call that asserts narrows its subject binding. The map
/// exists only because no stub declares these functions yet; folding it
/// into the stubs is blocked on r-typeshed (issue #41).
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

/// Record in `scope` that `var` is non-null: narrow its binding away from
/// NULL and mark the name branch-local. Does nothing when the binding has
/// no NULL member to remove.
fn narrow_away_from_null_in(scope: &mut Scope, var: &str, narrowed: &mut HashSet<String>) {
    if let Some(existing) = scope.get(var).cloned()
        && let Some(n) = narrow_away_from_null(&existing)
    {
        scope.insert_narrowed(var.to_string(), n);
        narrowed.insert(var.to_string());
    }
}

/// Apply a narrowing to produce separate scopes for the `then` and
/// `else_` branches. Returns `(then_scope, else_scope)` where each is
/// a clone of `base` with the appropriate binding updated.
///
pub(crate) fn apply_narrowing(
    base: &Scope,
    narrowing: &Narrowing,
) -> (Scope, Scope, HashSet<String>) {
    if matches!(narrowing, Narrowing::None) {
        return (base.clone(), base.clone(), HashSet::new());
    }
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
            // A mode-only predicate never rewrites a KNOWN type
            // (`is.numeric` on Integer must not become Double);
            // class targets and incompatible parameter defaults
            // do install.
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
                narrow_away_from_null_in(&mut else_scope, var, &mut narrowed);
            }
        }
        Narrowing::Negative { var, target } => {
            // The true branch of a negated null predicate is non-null. Other
            // complements are not representable in the current lattice, so
            // leave them conservative and retain the useful else fact below.
            if target.mode == Mode::Null {
                narrow_away_from_null_in(&mut then_scope, var, &mut narrowed);
            }
            install_positive_narrowing(&mut else_scope, var, target, &mut narrowed);
        }
        Narrowing::NonNullElse { var } => {
            narrow_away_from_null_in(&mut else_scope, var, &mut narrowed);
        }
        Narrowing::Else { var, target } => {
            debug_assert_eq!(target.mode, Mode::Null);
            narrow_away_from_null_in(&mut else_scope, var, &mut narrowed);
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
    let left = modes_of(left);
    let right = modes_of(right);
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

/// Walk `stmts` collecting calls inside `caller`'s body that forward its
/// `params` to nested calls. Skips nested function bodies: a nested
/// function has its own formals and is collected separately when it has
/// a binding.
pub(crate) fn collect_forwarded_calls_in_stmts(
    caller: &str,
    params: &[Param],
    stmts: &[Stmt],
    calls: &mut Vec<ForwardedCall>,
) {
    let _ = walk_stmts(
        stmts,
        Walk {
            fn_bodies: false,
            ..Walk::ALL
        },
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            if let AstNode::Expr(Expr::Call { func, args, .. }) = node
                && let Expr::Ident { name, .. } = func.as_ref()
            {
                let callee = crate::semantic_lists::bare_name(name);
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
            ControlFlow::Continue(Descend::Into)
        },
    );
}

impl Checker {
    /// Cached defusing helpers. The set is derived from `fn_table.fns`, whose
    /// quoting and defusing facts keep moving after collection: the fixpoint
    /// walks (which build this cache) run before each round of quoting
    /// propagation. The cache is therefore cleared by `collect_fns` and by
    /// every later writer of those flags (`propagate_s3_generic_quoting`,
    /// `propagate_forwarded_quoting`, `seed_caller_visible_signatures`), so a
    /// rebuilt set always reflects the post-propagation table.
    fn trusted_defusers(&mut self) -> Arc<HashSet<String>> {
        if let Some(cached) = &self.trusted_defusers {
            return Arc::clone(cached);
        }
        let built = Arc::new(build_trusted_defusers(&self.fn_table));
        self.trusted_defusers = Some(Arc::clone(&built));
        built
    }

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
        let trusted_defusers = self.trusted_defusers();

        for param in params {
            let Some(default) = &param.default else {
                continue;
            };

            // A formal promise shadows every enclosing binding of the same
            // name. Diagnose a recursive default only when the body actually
            // forces that promise; defusing helpers such as enexpr()/enquo()
            // may deliberately capture `function(x = x)` without evaluating
            // the default.
            let forced_in_body =
                guaranteed_force_before_replacement(body, &param.name, &trusted_defusers);
            if forced_in_body
                && let Some(span) =
                    first_executed_identifier(default, &param.name, &trusted_defusers)
            {
                self.emit(
                    Severity::Warning,
                    span,
                    "RY098",
                    format!(
                        "parameter `{}` has a self-referential default that recurses when forced",
                        param.name
                    ),
                );
                continue;
            }

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
                    definitely_forced_identifier_in_stmt(statement, &param.name, &trusted_defusers)
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

/// The defusing call set RY098 trusts: the base allowlist, plus every
/// collected function whose formals are all quoting or defused. A collected
/// function of the same name that does not qualify drops the allowlist entry.
fn build_trusted_defusers(fn_table: &FnTable) -> HashSet<String> {
    let mut trusted: HashSet<String> = DEFUSING_CALLS.iter().map(|s| s.to_string()).collect();
    for (name, function) in &fn_table.fns {
        if function
            .params
            .iter()
            .all(|param| param.quoting || param.defused)
        {
            trusted.insert(name.clone());
        } else {
            trusted.remove(name);
        }
    }
    trusted
}

// The force/identifier family below (`guaranteed_force_before_replacement`,
// `definitely_forced_identifier{,_in_stmt}`, `first_executed_identifier{,_in_stmt}`)
// is deliberately NOT expressed through the shared walker in
// `ry_core::walk`: its rules select individual children of a node —
// call arguments are skipped unless the callee is a known strict
// builtin, an `if` with a literal condition visits only the taken
// branch, a `$` subscript's synthesized ident is skipped while the base
// is kept — and the walk must stop at the first identifier forced in
// evaluation order. That is an evaluation-order analysis with
// per-child laziness rules, not a subtree-skip policy, so it keeps its
// hand-rolled recursion.
fn guaranteed_force_before_replacement(
    body: &[Stmt],
    wanted: &str,
    trusted_defusers: &HashSet<String>,
) -> bool {
    for statement in body {
        match statement {
            Stmt::Assign {
                target: Expr::Ident { name, .. },
                value,
                ..
            } if name == wanted => {
                return definitely_forced_identifier(value, wanted, trusted_defusers).is_some();
            }
            _ => {}
        }
        if definitely_forced_identifier_in_stmt(statement, wanted, trusted_defusers).is_some() {
            return true;
        }
        if matches!(statement, Stmt::If { .. }) {
            // A non-literal condition may take a diverging branch, so later
            // statements are not guaranteed to execute.
            return false;
        }
        let explicit_return = matches!(statement, Stmt::Return { .. })
            || matches!(
                statement,
                Stmt::Expr(Expr::Call { func, .. })
                    if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "return")
            );
        if explicit_return {
            return false;
        }
    }
    false
}

/// Find a force that is guaranteed when this statement executes. Conditional
/// branch bodies and loop bodies are not guaranteed to run; their conditions
/// (and a for-loop's iterator) are.
fn definitely_forced_identifier_in_stmt(
    statement: &Stmt,
    wanted: &str,
    trusted_defusers: &HashSet<String>,
) -> Option<Span> {
    match statement {
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            definitely_forced_identifier(value, wanted, trusted_defusers)
        }
        Stmt::If {
            cond, then, else_, ..
        } => match cond {
            Expr::Logical(true, span) => {
                guaranteed_force_before_replacement(then, wanted, trusted_defusers).then_some(*span)
            }
            Expr::Logical(false, span) => else_.as_ref().and_then(|statements| {
                guaranteed_force_before_replacement(statements, wanted, trusted_defusers)
                    .then_some(*span)
            }),
            _ => definitely_forced_identifier(cond, wanted, trusted_defusers),
        },
        Stmt::While { cond, .. } => definitely_forced_identifier(cond, wanted, trusted_defusers),
        Stmt::For { iter, .. } => definitely_forced_identifier(iter, wanted, trusted_defusers),
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|value| definitely_forced_identifier(value, wanted, trusted_defusers)),
        Stmt::FunctionDef { .. } => None,
    }
}

fn definitely_forced_identifier(
    expr: &Expr,
    wanted: &str,
    trusted_defusers: &HashSet<String>,
) -> Option<Span> {
    match expr {
        Expr::If {
            cond, then, else_, ..
        } => match cond.as_ref() {
            Expr::Logical(true, _) => definitely_forced_identifier(then, wanted, trusted_defusers),
            Expr::Logical(false, _) => else_
                .as_ref()
                .and_then(|else_| definitely_forced_identifier(else_, wanted, trusted_defusers)),
            _ => definitely_forced_identifier(cond, wanted, trusted_defusers),
        },
        Expr::BinOp {
            lhs,
            op: BinOpKind::AndAnd | BinOpKind::OrOr,
            ..
        } => definitely_forced_identifier(lhs, wanted, trusted_defusers),
        Expr::Block { body, span } => {
            guaranteed_force_before_replacement(body, wanted, trusted_defusers).then_some(*span)
        }
        _ => first_executed_identifier(expr, wanted, trusted_defusers),
    }
}

fn first_executed_identifier_in_stmt(
    statement: &Stmt,
    wanted: &str,
    trusted_defusers: &HashSet<String>,
) -> Option<Span> {
    match statement {
        Stmt::Assign { value, .. } => first_executed_identifier(value, wanted, trusted_defusers),
        Stmt::Expr(expr) => first_executed_identifier(expr, wanted, trusted_defusers),
        Stmt::If {
            cond, then, else_, ..
        } => first_executed_identifier(cond, wanted, trusted_defusers)
            .or_else(|| {
                then.iter().find_map(|statement| {
                    first_executed_identifier_in_stmt(statement, wanted, trusted_defusers)
                })
            })
            .or_else(|| {
                else_.as_ref().and_then(|statements| {
                    statements.iter().find_map(|statement| {
                        first_executed_identifier_in_stmt(statement, wanted, trusted_defusers)
                    })
                })
            }),
        Stmt::For { iter, body, .. } => first_executed_identifier(iter, wanted, trusted_defusers)
            .or_else(|| {
                body.iter().find_map(|statement| {
                    first_executed_identifier_in_stmt(statement, wanted, trusted_defusers)
                })
            }),
        Stmt::While { cond, body, .. } => first_executed_identifier(cond, wanted, trusted_defusers)
            .or_else(|| {
                body.iter().find_map(|statement| {
                    first_executed_identifier_in_stmt(statement, wanted, trusted_defusers)
                })
            }),
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|value| first_executed_identifier(value, wanted, trusted_defusers)),
        // Defining a closure does not evaluate its body or force captures.
        Stmt::FunctionDef { .. } => None,
    }
}

fn first_executed_identifier(
    expr: &Expr,
    wanted: &str,
    trusted_defusers: &HashSet<String>,
) -> Option<Span> {
    match expr {
        Expr::Ident { name, span } => (name == wanted).then_some(*span),
        Expr::Call { func, args, .. } => {
            let callee = ident_name(func);
            if callee.is_some_and(|name| {
                let qualified_defuser = name.rsplit_once("::").is_some_and(|(package, _)| {
                    matches!(package.trim_end_matches(':'), "base" | "rlang")
                        && DEFUSING_CALLS.contains(&crate::semantic_lists::bare_name(name))
                });
                name == "missing" || qualified_defuser || trusted_defusers.contains(name)
            }) {
                first_executed_identifier(func, wanted, trusted_defusers)
            } else if callee.is_some_and(|name| {
                // Only explicitly qualified strict builtins establish
                // guaranteed argument forcing. Bare names may be shadowed by
                // lazy user functions.
                name.rsplit_once("::").is_some_and(|(package, bare)| {
                    matches!(package.trim_end_matches(':'), "base" | "rlang")
                        && matches!(bare, "abort" | "stop" | "warning" | "message")
                })
            }) {
                first_executed_identifier(func, wanted, trusted_defusers).or_else(|| {
                    args.iter().find_map(|argument| {
                        first_executed_identifier(&argument.value, wanted, trusted_defusers)
                    })
                })
            } else {
                first_executed_identifier(func, wanted, trusted_defusers)
            }
        }
        Expr::BinOp { lhs, rhs, op, .. } => {
            if matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                first_executed_identifier(rhs, wanted, trusted_defusers)
            } else {
                first_executed_identifier(lhs, wanted, trusted_defusers)
                    .or_else(|| first_executed_identifier(rhs, wanted, trusted_defusers))
            }
        }
        Expr::UnaryOp { expr, .. } => first_executed_identifier(expr, wanted, trusted_defusers),
        Expr::Index {
            base, kind, args, ..
        } => first_executed_identifier(base, wanted, trusted_defusers).or_else(|| {
            // `$field` stores `field` as a synthesized identifier in the AST,
            // but R does not evaluate it as an expression. Counting that name
            // would turn `vars = parent$vars` into a self-reference.
            (!matches!(kind, IndexKind::Dollar)).then(|| {
                args.iter().find_map(|argument| {
                    first_executed_identifier(&argument.value, wanted, trusted_defusers)
                })
            })?
        }),
        Expr::Block { body, .. } => body.iter().find_map(|statement| {
            first_executed_identifier_in_stmt(statement, wanted, trusted_defusers)
        }),
        Expr::If {
            cond, then, else_, ..
        } => first_executed_identifier(cond, wanted, trusted_defusers)
            .or_else(|| first_executed_identifier(then, wanted, trusted_defusers))
            .or_else(|| {
                else_
                    .as_ref()
                    .and_then(|else_| first_executed_identifier(else_, wanted, trusted_defusers))
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

/// Identifiers the expression evaluates when forced. Skips assignment
/// targets (R does not evaluate them), `$` subscript idents, and nested
/// function bodies.
fn collect_executed_identifiers(expr: &Expr, names: &mut HashSet<String>) {
    let _ = walk_expr(
        expr,
        Walk {
            assign_targets: false,
            assign_operands: false,
            dollar_args: false,
            fn_bodies: false,
            ..Walk::ALL
        },
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            if let AstNode::Expr(Expr::Ident { name, .. }) = node {
                names.insert(name.clone());
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
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

impl ArgumentMatch {
    /// The actual argument bound to formal parameter `formal_index`,
    /// if any: the reverse of `param_for_arg`. Repeated names
    /// (`f(x = 1, x = 2)`, a runtime error in R) bind the first actual.
    pub(crate) fn arg_for_param(&self, formal_index: usize) -> Option<usize> {
        self.param_for_arg
            .iter()
            .position(|bound| *bound == Some(formal_index))
    }
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

/// The formal-parameter view RY090/RY091 reporting needs, shared by
/// typeshed `ParamSpec`s and collected `UserParam`s so one code path
/// serves stub and user calls.
trait CallFormal {
    fn name(&self) -> &str;
    fn required(&self) -> bool;
}

impl CallFormal for ParamSpec {
    fn name(&self) -> &str {
        &self.name
    }
    fn required(&self) -> bool {
        self.required
    }
}

impl CallFormal for UserParam {
    fn name(&self) -> &str {
        &self.name
    }
    fn required(&self) -> bool {
        self.required
    }
}

/// `match_arguments` over a signature's formal specs: collect the formal
/// names once and run R's three-pass matching. Callers that need the
/// names themselves (message text, eval-mode lookup) keep their own
/// `param_names()` vector.
pub(crate) fn match_params(params: &[ParamSpec], args: &[Arg]) -> ArgumentMatch {
    let names: Vec<&str> = params.iter().map(|param| param.name.as_str()).collect();
    match_arguments(&names, args)
}

/// Return the actual argument bound to a formal under ordinary R matching.
/// Semantic metadata must use this rather than raw call positions.
pub(crate) fn bound_argument_index(
    params: &[ParamSpec],
    args: &[Arg],
    formal: &str,
) -> Option<usize> {
    bound_argument_index_matched(params, &match_params(params, args), formal)
}

/// `bound_argument_index` over an argument match already computed for
/// this call, so a site that consults several formals matches once.
pub(crate) fn bound_argument_index_matched(
    params: &[ParamSpec],
    bindings: &ArgumentMatch,
    formal: &str,
) -> Option<usize> {
    bindings.arg_for_param(params.iter().position(|param| param.name == formal)?)
}

pub(crate) fn match_args_to_params(
    sig_params: &[ParamSpec],
    args: &[Arg],
    arg_types: &[RType],
) -> Vec<RType> {
    let bindings = match_params(sig_params, args);
    let mut matched = vec![RType::unknown(); sig_params.len()];
    for (formal_index, slot) in matched.iter_mut().enumerate() {
        if let Some(argument_type) = bindings
            .arg_for_param(formal_index)
            .and_then(|argument_index| arg_types.get(argument_index))
        {
            *slot = argument_type.clone();
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
        let bindings = match_params(&signature.params, args);
        // `...` accepts every otherwise-unmatched actual argument. Without
        // it, report only named arguments; excess positionals are outside
        // this rule's deliberately narrow scope.
        let supports_unknown_argument_check = signature
            .params
            .iter()
            .any(|param| param.required || param.default.is_some() || param.type_.is_some());
        self.check_call_arity(
            function_name,
            &signature.params,
            args,
            &bindings,
            supports_unknown_argument_check,
            call_span,
        );

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
            if generic_argument_may_dispatch(&self.typeshed.globals, function_name, actual) {
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
        self.check_call_arity(
            function_name,
            &function.params,
            args,
            &bindings,
            true,
            call_span,
        );
    }

    /// Shared arity reporting over one argument match: RY090 for named
    /// actuals no formal matched, RY091 for required formals no actual
    /// bound. The typeshed and user-function checks differ only in their
    /// formals source and unknown-argument gating.
    fn check_call_arity<P: CallFormal>(
        &mut self,
        function_name: &str,
        params: &[P],
        args: &[Arg],
        bindings: &ArgumentMatch,
        report_unknown: bool,
        call_span: Span,
    ) {
        let names: Vec<&str> = params.iter().map(|param| param.name()).collect();
        let required: Vec<bool> = params.iter().map(|param| param.required()).collect();
        self.emit_unknown_arguments(function_name, &names, args, bindings, report_unknown);
        self.emit_missing_required(function_name, &names, &required, bindings, call_span);
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
            let message = format!("unknown argument `{argument_name}` to `{function_name}`{hint}");
            self.emit(Severity::Warning, argument.span, "RY090", message);
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
    let mut closest = None;
    let mut minimum_distance = usize::MAX;
    let mut minimum_is_tied = false;

    for parameter in parameters.iter().copied().filter(|name| *name != "...") {
        let distance = edit_distance(argument, parameter);
        if distance > 2 {
            continue;
        }
        match distance.cmp(&minimum_distance) {
            std::cmp::Ordering::Less => {
                closest = Some(parameter);
                minimum_distance = distance;
                minimum_is_tied = false;
            }
            std::cmp::Ordering::Equal => minimum_is_tied = true,
            std::cmp::Ordering::Greater => {}
        }
    }

    if minimum_is_tied { None } else { closest }
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
    let Some(actual_modes) = mode_set(actual) else {
        return false;
    };
    let Some(expected_modes) = mode_set(expected) else {
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
    /// `modes_of`'s walk keeping full `RType`s: standalone assertions
    /// are exact, so each member's length and class are preconditions
    /// too. A member-less union degrades to the union itself.
    fn members(rtype: &RType) -> Vec<&RType> {
        if rtype.mode == Mode::Union {
            match rtype.members.as_deref() {
                Some(union_members) => union_members.iter().flat_map(|m| members(m)).collect(),
                None => vec![rtype],
            }
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

/// Whether `function_name` is an S3 generic whose stub parameter types
/// method dispatch can defeat: a classed or NULL argument may route to a
/// method that accepts it, so RY092 stays quiet. The names come from the
/// same two sources the dispatch path in `infer_call` consults: the base
/// stub's `globals.s3_generics` and the registered group-generic member
/// lists ([`crate::semantic_lists::S3_MATH_GENERICS`] and
/// [`crate::semantic_lists::S3_SUMMARY_GENERICS`], which cover
/// `round`, `log`, `sqrt`, and `exp`). `mean` stays a documented special
/// case: it is a plain S3 generic (a `mean.<class>` method catches it,
/// but a `Summary.<class>` method does not), so it belongs in neither
/// group list, and the base stub's `globals.s3_generics` omits it.
/// r-typeshed registering it there lets this fallback shrink (issue #41).
fn generic_argument_may_dispatch(
    globals: &ry_typeshed::Globals,
    function_name: &str,
    actual: &RType,
) -> bool {
    let generic = globals
        .s3_generics
        .iter()
        .any(|generic| generic == function_name)
        || crate::higher_order::s3_group_generic(function_name).is_some()
        || function_name == "mean";
    generic && (actual.class.has_known_class() || actual.mode == Mode::Null)
}

fn compatible_mode_pair(actual: Mode, expected: Mode) -> bool {
    actual == expected || (numeric_family(actual) && numeric_family(expected))
}

pub(crate) fn expected_type_label(expected: &RType) -> String {
    let Some(modes) = mode_set(expected) else {
        return "unknown".to_string();
    };
    // "numeric" is message wording, not a coercion promise: it covers
    // the wider logical..complex ladder R's own docs call numeric (see
    // `numeric_family`).
    if modes.len() >= 3
        && modes
            .iter()
            .all(|mode| numeric_family(*mode) || matches!(mode, Mode::Complex))
    {
        return "numeric".to_string();
    }
    modes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" or ")
}

/// Eval mode declared for the argument at `index`.
///
/// `bindings` comes from one `match_params` call shared by the whole call
/// site, so a loop over arguments does not re-match per argument.
pub(crate) fn eval_mode_for_arg(
    sig: &FunctionSig,
    bindings: &ArgumentMatch,
    index: usize,
) -> Option<EvalMode> {
    let parameter = bindings
        .param_for_arg
        .get(index)?
        .and_then(|parameter_index| sig.params.get(parameter_index))
        .map(|param| param.name.as_str())
        .unwrap_or("...");
    sig.eval
        .get(parameter)
        .copied()
        .or_else(|| sig.eval.get("...").copied())
}

pub(crate) fn argument_eval_mode(
    sig: &FunctionSig,
    args: &[Arg],
    index: usize,
) -> Option<EvalMode> {
    eval_mode_for_arg(sig, &match_params(&sig.params, args), index)
}

/// Locate the supplied argument named by a signature's data-mask source.
/// Formula APIs place `data` after their quoted formula, and some calls put it
/// after mask-evaluated arguments, so callers must not assume argument zero.
pub(crate) fn data_mask_source_arg(sig: &FunctionSig, args: &[Arg]) -> Option<usize> {
    let source = sig.data_mask_source.as_deref()?;
    let bindings = match_params(&sig.params, args);
    bound_argument_index_matched(&sig.params, &bindings, source)
}

/// If `e` is a literal expression (`42`, `"x"`, `TRUE`, `NULL`, `NA`),
/// return the mode that calling it would error with.
/// Non-literal callees return `None` so the caller stays silent.
pub(crate) fn literal_callee_mode(e: &Expr) -> Option<Mode> {
    let t = infer_literal_default(e);
    (!matches!(t.mode, Mode::Opaque)).then_some(t.mode)
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
    debug_assert_eq!(schema.columns.len(), args.len());
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
        .map(|(name, child)| (name.clone(), json_rtype_scalar(child)))
        .collect();
    let schema = Arc::new(ColumnSchema {
        columns: cols,
        complete: true,
        locally_constructed: false,
    });
    base.with_columns(schema)
}

/// Map a parsed `JsonLength` to the checker's `Length`. Literal lengths
/// map exactly; every arg-derived spec (`arg0`, `longest_arg`, ...) and a
/// missing spec map to `Length::Unknown`, so callers that resolve those
/// specs contextually must do so before falling back to this.
pub(crate) fn json_length_to_length(spec: Option<JsonLength>) -> Length {
    match spec {
        Some(JsonLength::Known(0)) => Length::Zero,
        Some(JsonLength::Known(1)) => Length::One,
        Some(JsonLength::Known(value)) => Length::Known(value),
        _ => Length::Unknown,
    }
}

pub(crate) fn json_rtype_scalar(jt: &JsonRType) -> RType {
    let length = json_length_to_length(JsonLength::parse(&jt.length));
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
    let mode = concrete_json_mode(&jt.mode).unwrap_or(Mode::Opaque);
    let class = if jt.class.is_empty() {
        ClassVector::empty()
    } else {
        let refs: Vec<&str> = jt.class.iter().map(|s| s.as_str()).collect();
        ClassVector::from_slice(&refs)
    };
    RType::new(mode, length).with_class(class)
}

/// Map a typeshed mode string to the concrete `Mode` it names. Returns
/// `None` for `union`, the compound arg-derived specs, and unrecognized
/// strings; callers decide their own fallback for those.
pub(crate) fn concrete_json_mode(mode: &str) -> Option<Mode> {
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

impl Checker {
    /// Signature-declared predicates extend the built-in predicate vocabulary
    /// only when ordinary typeshed resolution establishes their provenance.
    pub(crate) fn extract_type_narrowing(&self, cond: &Expr, scope: &Scope) -> Narrowing {
        let built_in = extract_builtin_type_narrowing(cond);
        if !matches!(built_in, Narrowing::None) {
            return built_in;
        }
        if let Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } = cond
        {
            return match self.extract_type_narrowing(expr, scope) {
                Narrowing::Positive { var, target } => Narrowing::Negative { var, target },
                Narrowing::Negative { var, target } => Narrowing::Positive { var, target },
                _ => Narrowing::None,
            };
        }
        let Expr::Call { func, args, .. } = cond else {
            return Narrowing::None;
        };
        let Expr::Ident { name, .. } = func.as_ref() else {
            return Narrowing::None;
        };
        // Predicate facts require the same provenance as ordinary calls.
        // A local value/function shadows a bare stub, while a qualified
        // name is resolved only in its explicit package.
        if !name.contains("::") && scope.get(name).is_some() && scope.function_alias(name).is_none()
        {
            return Narrowing::None;
        }
        let Some(signature) = self.resolve_predicate_sig(name) else {
            return Narrowing::None;
        };
        let Some(predicate) = signature.predicate else {
            return Narrowing::None;
        };
        let Some(subject_index) =
            bound_argument_index(&signature.params, args, &predicate.subject_param)
        else {
            return Narrowing::None;
        };
        let Some(Expr::Ident { name: var, .. }) = args.get(subject_index).map(|arg| &arg.value)
        else {
            return Narrowing::None;
        };
        Narrowing::Positive {
            var: var.clone(),
            target: json_rtype_to_rtype(&predicate.target),
        }
    }
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
    fn formal_lookup_supports_positional_exact_and_partial_matching() {
        let params = ["file", "local", "..."]
            .into_iter()
            .map(|name| ParamSpec {
                name: name.to_string(),
                type_: None,
                required: false,
                default: None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bound_argument_index(&params, &[argument(None), argument(None)], "local"),
            Some(1)
        );
        assert_eq!(
            bound_argument_index(
                &params,
                &[argument(Some("local")), argument(Some("file"))],
                "local"
            ),
            Some(0)
        );
        assert_eq!(
            bound_argument_index(&params, &[argument(Some("lo"))], "local"),
            Some(0)
        );
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

/// Pins for the mode/union-set helpers unified in issue #169:
/// `modes_compatible`, `types_intersect`,
/// `types_provably_incompatible`/`expected_type_label`,
/// `standalone_check_provably_rejects`, and the narrowing call sites.
/// Each row is a decision a call site relies on.
#[cfg(test)]
mod mode_union_set_pins {
    use super::*;

    /// Test-only union of mode-only scalars (length One, no class); must not be used where length/class matter (standalone/narrowing cases keep bespoke constructions).
    fn union(modes: &[Mode]) -> RType {
        RType::union(Arc::from_iter(
            modes.iter().map(|mode| RType::scalar(*mode)),
        ))
    }

    #[test]
    fn modes_compatible_pins_the_coercion_view() {
        for (actual, target, compatible) in [
            // No evidence: opaque/union/null callback returns stay compatible.
            (Mode::Opaque, Mode::Double, true),
            (Mode::Union, Mode::Double, true),
            (Mode::Null, Mode::Double, true),
            // The silent-coercion family interchanges freely.
            (Mode::Logical, Mode::Double, true),
            (Mode::Integer, Mode::Logical, true),
            (Mode::Double, Mode::Integer, true),
            // The footguns RY080 exists for.
            (Mode::Character, Mode::Double, false),
            (Mode::Double, Mode::Character, false),
            // Complex into a numeric target discards imaginary parts with
            // a warning, so it is not silently compatible; unmodeled
            // targets stay permissive.
            (Mode::Complex, Mode::Double, false),
            (Mode::Double, Mode::Complex, true),
        ] {
            assert_eq!(
                modes_compatible(&actual, &target),
                compatible,
                "{actual:?} into {target:?}"
            );
        }
    }

    #[test]
    fn types_intersect_pins_mode_overlap() {
        let double = RType::scalar(Mode::Double);
        let character = RType::scalar(Mode::Character);
        let numeric_union = union(&[Mode::Integer, Mode::Double]);
        let opaque_member_union = union(&[Mode::Character, Mode::Opaque]);
        for (left, right, overlap) in [
            (&double, &double, true),
            (&character, &double, false),
            (&numeric_union, &double, true),
            (&double, &numeric_union, true),
            (&numeric_union, &character, false),
            // Opaque is a mode matching only itself: opaque-vs-double is
            // disjoint -- what lets a guarded default parameter be replaced
            // by the predicate's type -- and an opaque-bearing union still
            // intersects through its knowable members.
            (&RType::unknown(), &double, false),
            (&double, &RType::unknown(), false),
            (&RType::unknown(), &RType::unknown(), true),
            (&opaque_member_union, &double, false),
            (&opaque_member_union, &character, true),
        ] {
            assert_eq!(
                types_intersect(left, right),
                overlap,
                "{left:?} vs {right:?}"
            );
        }
    }

    #[test]
    fn types_provably_incompatible_pins_argument_checking() {
        let double = RType::scalar(Mode::Double);
        let character = RType::scalar(Mode::Character);
        for (actual, expected, incompatible) in [
            (&character, &double, true),
            (&RType::scalar(Mode::Integer), &double, false),
            (&double, &RType::scalar(Mode::Logical), false),
            // Complex is outside the silent-coercion family.
            (&RType::scalar(Mode::Complex), &double, true),
            // Opaque anywhere means "cannot decide", never a mismatch.
            (&RType::unknown(), &double, false),
            (&double, &RType::unknown(), false),
            (&union(&[Mode::Double, Mode::Opaque]), &character, false),
            // A union with any compatible member passes.
            (&union(&[Mode::Character, Mode::Double]), &double, false),
            (&RType::scalar(Mode::List), &double, true),
        ] {
            assert_eq!(
                types_provably_incompatible(actual, expected),
                incompatible,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn expected_type_label_pins_message_text() {
        for (expected, label) in [
            (&RType::scalar(Mode::Double), "double"),
            (&RType::unknown(), "unknown"),
            (
                &union(&[Mode::Logical, Mode::Integer, Mode::Double]),
                "numeric",
            ),
            // The label's one deliberate divergence from `numeric_family`:
            // "numeric" is wording for unions R's own documentation calls
            // numeric, so complex is included.
            (
                &union(&[Mode::Integer, Mode::Double, Mode::Complex]),
                "numeric",
            ),
        ] {
            assert_eq!(expected_type_label(expected), label, "{expected:?}");
        }
    }

    #[test]
    fn standalone_check_pins_exact_assertions() {
        let double = RType::scalar(Mode::Double);
        let character = RType::scalar(Mode::Character);
        let nullable_character = RType::union(Arc::from(vec![
            RType::new(Mode::Null, Length::Zero),
            character.clone(),
        ]));
        for (actual, expected, rejects) in [
            // Exact assertions: numeric modes are not interchangeable
            // (unlike ordinary argument compatibility), and length is a
            // runtime precondition. Opaque never rejects.
            (&character, &double, true),
            (&double, &double, false),
            (&RType::scalar(Mode::Integer), &double, true),
            (&double, &RType::new(Mode::Double, Length::Known(2)), true),
            (&double, &RType::new(Mode::Double, Length::Unknown), false),
            (&RType::unknown(), &double, false),
            // A union expectation with any overlapping member passes.
            (&character, &nullable_character, false),
            (&double, &nullable_character, true),
        ] {
            assert_eq!(
                standalone_check_provably_rejects(actual, expected),
                rejects,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    // Pins installed-narrowing outcomes rather than the compatibility
    // flag itself: the opaque-default arms are disjoint from the guard
    // (not merely "unprovable"), pinning the disjoint-default install
    // path, and a plain union binding narrows to the confirmed member.
    #[test]
    fn guards_install_over_opaque_defaults_and_confirmed_members() {
        let target = RType::scalar(Mode::Double);
        for (as_default, existing) in [
            (true, RType::unknown()),
            (true, union(&[Mode::Character, Mode::Opaque])),
            (false, union(&[Mode::Integer, Mode::Double])),
        ] {
            // Discriminate the disjointness claim: the opaque-default arms
            // are disjoint from the guard (opaque matches only itself),
            // while the confirmed-member arm overlaps through Double.
            if as_default {
                assert!(
                    !types_intersect(&existing, &target),
                    "opaque default must be disjoint from the guard: {existing:?}"
                );
            } else {
                assert!(
                    types_intersect(&existing, &target),
                    "confirmed-member union must overlap the guard: {existing:?}"
                );
            }
            let note = format!("guard must install its type over {existing:?}");
            let mut base = Scope::default();
            if as_default {
                base.insert_parameter_default("x", existing);
            } else {
                base.insert("x", existing);
            }
            let (then_scope, ..) = apply_narrowing(
                &base,
                &Narrowing::Positive {
                    var: "x".to_string(),
                    target: target.clone(),
                },
            );
            assert_eq!(
                then_scope.get("x").map(|t| t.mode),
                Some(Mode::Double),
                "{note}"
            );
        }
    }
}
