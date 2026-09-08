//! Type facts established by guards and assertions.

use super::*;

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

/// Record that `var` is non-null, returning whether its type was narrowed.
fn narrow_away_from_null_in(scope: &mut Scope, var: &str) -> bool {
    if let Some(existing) = scope.get(var).cloned()
        && let Some(n) = narrow_away_from_null(&existing)
    {
        scope.insert_narrowed(var.to_string(), n);
        return true;
    }
    false
}

#[derive(Clone, Copy)]
pub(crate) enum NarrowingBranch {
    Then,
    Else,
}

/// Clone both branches when their independent outcomes must be merged.
/// The returned names identify branch-local refinements that assignment merging
/// must exclude from the parent scope.
#[cfg(test)]
pub(crate) fn apply_narrowing(
    base: &Scope,
    narrowing: &Narrowing,
) -> (Scope, Scope, HashSet<String>) {
    let (mut then_scope, mut else_scope) = (base.clone(), base.clone());
    let then_name = apply_narrowing_branch(&mut then_scope, narrowing, NarrowingBranch::Then);
    let else_name = apply_narrowing_branch(&mut else_scope, narrowing, NarrowingBranch::Else);
    let narrowed = then_name
        .into_iter()
        .chain(else_name)
        .map(str::to_owned)
        .collect();
    (then_scope, else_scope, narrowed)
}

/// Whether every mode `existing` may take is one of `target`'s tested modes,
/// so the predicate's false path excludes the whole recorded type. A union
/// target (`is.numeric`) tests each of its member modes.
fn type_is_exactly_tested_family(existing: &RType, target: &RType) -> bool {
    let tested = |mode: Mode| -> bool {
        match target.mode {
            Mode::Union => target
                .members
                .as_ref()
                .is_some_and(|members| members.iter().any(|member| member.mode == mode)),
            _ => mode == target.mode,
        }
    };
    match existing.mode {
        Mode::Union => existing.members.as_ref().is_some_and(|members| {
            !members.is_empty() && members.iter().all(|member| tested(member.mode))
        }),
        mode => tested(mode),
    }
}

/// Apply only the selected path's refinement. Assertions keep this scope;
/// conditional expressions supply a single isolated clone for their RHS.
pub(crate) fn apply_narrowing_branch<'a>(
    scope: &mut Scope,
    narrowing: &'a Narrowing,
    branch: NarrowingBranch,
) -> Option<&'a str> {
    match (narrowing, branch) {
        (Narrowing::Positive { var, target }, NarrowingBranch::Then) => {
            // A mode-only predicate never rewrites a KNOWN type
            // (`is.numeric` on Integer must not become Double);
            // class targets and incompatible parameter defaults
            // do install.
            if let Some(existing) = scope.get(var).cloned() {
                let class_narrowing = target.class.has_known_class();
                let incompatible_parameter_default =
                    scope.is_default_parameter(var) && !types_intersect(&existing, target);
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
                    scope.insert_narrowed(
                        var.clone(),
                        RType {
                            mode: target.mode,
                            length: existing.length,
                            ..target.clone()
                        },
                    );
                    return Some(var.as_str());
                }
            }
        }
        (Narrowing::Positive { var, target }, NarrowingBranch::Else)
        | (Narrowing::Negative { var, target }, NarrowingBranch::Then) => {
            // Only NULL has a representable negative-path complement.
            if target.mode == Mode::Null && narrow_away_from_null_in(scope, var) {
                return Some(var.as_str());
            }
            // A defaulted parameter's recorded type describes only the
            // omitted-argument call shape (`diagnostic_parameter_type`).
            // When a mode predicate's false path proves the runtime value
            // is not any tested mode, the default-derived mode cannot
            // describe it here; degrade to unknown in this branch only.
            // Mode predicates carry a concrete or union mode and no class
            // claim — `Mode::Opaque` targets are class predicates
            // (`inherits`, `is.environment`, `is.<class>`), whose false
            // path rejects no mode. Locals keep their type (their rejected
            // branch is genuinely unreachable), mixed unions keep any
            // surviving member, and a rebinding clears the default-
            // parameter marker before this can apply.
            if matches!(
                target.mode,
                Mode::Logical
                    | Mode::Integer
                    | Mode::Double
                    | Mode::Character
                    | Mode::Complex
                    | Mode::Raw
                    | Mode::List
                    | Mode::Function
                    | Mode::Union
            ) && scope.is_default_parameter(var)
                && let Some(existing) = scope.get(var)
                && type_is_exactly_tested_family(existing, target)
            {
                scope.insert_narrowed(var.clone(), RType::unknown());
                return Some(var.as_str());
            }
        }
        (Narrowing::Negative { var, target }, NarrowingBranch::Else) => {
            if install_positive_narrowing(scope, var, target) {
                return Some(var.as_str());
            }
        }
        (Narrowing::NonNullElse { var }, NarrowingBranch::Else) => {
            if narrow_away_from_null_in(scope, var) {
                return Some(var.as_str());
            }
        }
        (Narrowing::Else { var, target }, NarrowingBranch::Else) => {
            debug_assert_eq!(target.mode, Mode::Null);
            if narrow_away_from_null_in(scope, var) {
                return Some(var.as_str());
            }
        }
        (Narrowing::ScalarElse { var, target }, NarrowingBranch::Else) => {
            if let Some(existing) = scope.get(var).cloned() {
                // A concrete NULL local cannot satisfy length(x) == 1. A
                // parameter default is not exhaustive: callers can pass a scalar.
                if existing.mode == Mode::Null && !scope.is_default_parameter(var) {
                    scope.unreachable = true;
                } else {
                    let mut scalar = match target {
                        Some(target) => target.clone(),
                        None if existing.mode == Mode::Null => RType::unknown(),
                        None => existing,
                    };
                    scalar.length = Length::One;
                    scope.insert_narrowed(var.clone(), scalar);
                    return Some(var.as_str());
                }
            }
        }
        _ => {}
    }
    None
}

fn install_positive_narrowing(scope: &mut Scope, var: &str, target: &RType) -> bool {
    let Some(existing) = scope.get(var).cloned() else {
        return false;
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
        return true;
    }
    false
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
mod selected_branch_tests {
    use super::*;

    #[test]
    fn exactly_tested_family_covers_groups_but_not_mixed_unions() {
        let character = RType::scalar(Mode::Character);
        let numeric = RType::scalar(Mode::Integer).join(RType::scalar(Mode::Double));
        let mixed = RType::scalar(Mode::Character).join(RType::scalar(Mode::Integer));
        // Single-mode predicate over the same mode.
        assert!(type_is_exactly_tested_family(&character, &character));
        // Group predicate over both member modes.
        assert!(type_is_exactly_tested_family(&numeric, &numeric));
        assert!(type_is_exactly_tested_family(
            &RType::scalar(Mode::Integer),
            &numeric
        ));
        // A mixed union survives the false path of a single-mode or group
        // predicate, so it is not "exactly tested".
        assert!(!type_is_exactly_tested_family(&mixed, &character));
        assert!(!type_is_exactly_tested_family(&mixed, &numeric));
        // A mode outside the tested family is not excluded.
        assert!(!type_is_exactly_tested_family(
            &RType::scalar(Mode::List),
            &character
        ));
    }

    #[test]
    fn positive_and_negated_union_guards_keep_their_distinct_rules() {
        let original = RType::union(Arc::from([
            RType::scalar(Mode::Integer),
            RType::scalar(Mode::Character),
        ]));
        let mut positive = Scope::default();
        positive.insert("x", original.clone());
        let mut negative = positive.clone();
        let target = RType::scalar(Mode::Double);
        // The positive guard requires an intersecting member. The false path
        // of the negated predicate has historically installed its target.
        assert_eq!(
            apply_narrowing_branch(
                &mut positive,
                &Narrowing::Positive {
                    var: "x".into(),
                    target: target.clone(),
                },
                NarrowingBranch::Then,
            ),
            None
        );
        assert_eq!(positive.get("x"), Some(&original));
        assert_eq!(
            apply_narrowing_branch(
                &mut negative,
                &Narrowing::Negative {
                    var: "x".into(),
                    target: target.clone(),
                },
                NarrowingBranch::Else,
            ),
            Some("x")
        );
        assert_eq!(negative.get("x").map(|ty| ty.mode), Some(Mode::Double));
    }

    #[test]
    fn in_place_narrowing_preserves_parameter_and_scope_metadata() {
        let mut scope = Scope::default().with_unknown_data_mask();
        scope.mark_search_path_unknown();
        scope.insert_parameter_default("x", RType::scalar(Mode::Null));
        scope.mark_list_origin("x");
        scope.set_function_alias("x", "base::identity".into());
        scope.mark_lexical_function("x");
        scope.insert_parameter("untouched", RType::scalar(Mode::Integer));
        scope.set_function_alias("untouched", "other".into());
        scope.mark_lexical_function("untouched");
        scope.tidy_injection = Some(InjectionMode::Full);
        apply_narrowing_branch(
            &mut scope,
            &Narrowing::Positive {
                var: "x".into(),
                target: RType::scalar(Mode::Double),
            },
            NarrowingBranch::Then,
        );
        assert_eq!(scope.get("x").map(|ty| ty.mode), Some(Mode::Double));
        assert!(scope.is_parameter("x") && scope.is_default_parameter("x"));
        assert!(scope.has_list_origin("x") && scope.narrowed_bindings.contains("x"));
        assert!(scope.function_alias("x").is_none() && !scope.is_lexical_function("x"));
        assert_eq!(
            scope.get("untouched").map(|ty| ty.mode),
            Some(Mode::Integer)
        );
        assert!(scope.is_parameter("untouched") && scope.is_lexical_function("untouched"));
        assert_eq!(scope.function_alias("untouched"), Some("other"));
        assert!(scope.data_mask_unknown && scope.search_path_unknown);
        assert_eq!(scope.tidy_injection, Some(InjectionMode::Full));
        assert!(!scope.unreachable);
    }

    #[test]
    fn selected_scalar_guard_distinguishes_null_locals_from_defaults() {
        for default_parameter in [false, true] {
            let mut scope = Scope::default();
            if default_parameter {
                scope.insert_parameter_default("x", RType::scalar(Mode::Null));
            } else {
                scope.insert("x", RType::scalar(Mode::Null));
            }
            let narrowing = Narrowing::ScalarElse {
                var: "x".into(),
                target: None,
            };
            assert_eq!(
                apply_narrowing_branch(&mut scope, &narrowing, NarrowingBranch::Then),
                None
            );
            assert!(!scope.unreachable);
            let changed = apply_narrowing_branch(&mut scope, &narrowing, NarrowingBranch::Else);
            assert_eq!(changed, default_parameter.then_some("x"));
            assert_eq!(scope.unreachable, !default_parameter);
            assert_eq!(
                scope.get("x").map(|ty| ty.mode),
                Some(if default_parameter {
                    Mode::Opaque
                } else {
                    Mode::Null
                })
            );
            if default_parameter {
                assert_eq!(scope.get("x").map(|ty| ty.length), Some(Length::One));
                assert!(scope.is_parameter("x") && scope.is_default_parameter("x"));
            }
        }
    }
}
