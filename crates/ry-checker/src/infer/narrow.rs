//! Type facts established by guards and assertions.

use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr};
use std::collections::BTreeSet;
use std::ops::ControlFlow;

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
    /// Each list contains facts proved on that path through a compound guard.
    Compound {
        base: Box<Narrowing>,
        on_true: Vec<Narrowing>,
        on_false: Vec<Narrowing>,
    },
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
    /// A zero-length guard (`!length(x)` or `length(x) == 0`) whose false
    /// path proves only that `x` is non-NULL.  This is deliberately weaker
    /// than claiming anything about its storage mode or non-emptiness.
    NonNullElse { var: String },
    /// A rejecting `||` chain containing `length(x) != 1`. Its false path
    /// proves that `x` has length one. A negated type predicate over the same
    /// variable (for example `!is.numeric(x)`) may additionally prove mode.
    ScalarElse { var: String, target: Option<RType> },
    /// A true guard proves that the loop-carried vector alternative cannot
    /// reach its body. This does not rewrite the binding's general RType.
    ScalarThen { var: String },
    /// `!missing(x)`: the caller supplied `x` in the then branch, so a
    /// defaulted parameter's omitted-call type does not describe it.
    Supplied { var: String },
    /// R never enters this branch with `var`'s recorded type, so code there
    /// sees an unknown value (base `is.numeric()` on a Date).
    Unreached { var: String },
}

impl Narrowing {
    /// Whether any fact in this narrowing is about one of `names`.
    pub(crate) fn mentions_any(&self, names: &HashSet<String>) -> bool {
        !names.is_empty() && self.mentions_where(&|var| names.contains(var))
    }

    /// Whether any fact in this narrowing is about `name`.
    pub(crate) fn mentions(&self, name: &str) -> bool {
        self.mentions_where(&|var| var == name)
    }

    fn mentions_where(&self, about: &dyn Fn(&str) -> bool) -> bool {
        match self {
            Narrowing::Compound {
                base,
                on_true,
                on_false,
            } => {
                base.mentions_where(about)
                    || on_true
                        .iter()
                        .chain(on_false)
                        .any(|narrowing| narrowing.mentions_where(about))
            }
            Narrowing::None => false,
            Narrowing::Positive { var, .. }
            | Narrowing::Negative { var, .. }
            | Narrowing::NonNullElse { var }
            | Narrowing::ScalarElse { var, .. }
            | Narrowing::ScalarThen { var }
            | Narrowing::Supplied { var }
            | Narrowing::Unreached { var } => about(var),
        }
    }

    /// This narrowing without its facts about `names`.
    fn without(self, names: &HashSet<String>) -> Narrowing {
        let keep = |facts: Vec<Narrowing>| -> Vec<Narrowing> {
            facts
                .into_iter()
                .map(|fact| fact.without(names))
                .filter(|fact| !matches!(fact, Narrowing::None))
                .collect()
        };
        match self {
            Narrowing::Compound {
                base,
                on_true,
                on_false,
            } => Narrowing::Compound {
                base: Box::new(base.without(names)),
                on_true: keep(on_true),
                on_false: keep(on_false),
            },
            fact if fact.mentions_any(names) => Narrowing::None,
            fact => fact,
        }
    }
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
                Some(RType::unknown().with_class(ClassVector::guard(class)))
            }
            _ => None,
        })
    } else {
        predicate_target(name).or_else(|| s3_predicate_target(name))
    }
}

pub(crate) fn extract_builtin_type_narrowing(cond: &Expr) -> Narrowing {
    if let Some(var) = non_empty_length_guard_var(cond) {
        return non_null_then(var);
    }
    match cond {
        Expr::Call { func, args, .. } => {
            let Expr::Ident { name, .. } = func.as_ref() else {
                return Narrowing::None;
            };
            if let Some(var) = missing_test_var(cond) {
                // Compound facts apply as then-branch facts: the else
                // branch of `missing(x)` has `x` supplied.
                return Narrowing::Compound {
                    base: Box::new(Narrowing::None),
                    on_true: Vec::new(),
                    on_false: vec![Narrowing::Supplied { var }],
                };
            }
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
            if let Some(var) = missing_test_var(expr) {
                return Narrowing::Supplied { var };
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
            // A false disjunction proves every disjunct false, so each
            // `is.null(x)` disjunct makes `x` non-NULL whatever the others
            // test (`if (is.null(x) || x == 1) return(NULL)`, #692): the
            // mirror of the `&&` arm below. A right operand that may assign
            // `x` voids the left operand's fact about it.
            let on_false: Vec<Narrowing> = non_null_else_facts(extract_builtin_type_narrowing(lhs))
                .into_iter()
                .filter(|fact| !fact.mentions_where(&|var| may_assign(rhs, var)))
                .chain(non_null_else_facts(extract_builtin_type_narrowing(rhs)))
                .collect();
            if on_false.is_empty() {
                return Narrowing::None;
            }
            Narrowing::Compound {
                base: Box::new(Narrowing::None),
                on_true: Vec::new(),
                on_false,
            }
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
            // comparisons such as `x %in% c("a", "b")`. Every conjunct
            // counts (`!is.null(a) && !is.null(b)` proves both), and a
            // false conjunction proves none.
            let on_true: Vec<Narrowing> = [lhs.as_ref(), rhs.as_ref()]
                .into_iter()
                .flat_map(|operand| non_null_then_facts(extract_builtin_type_narrowing(operand)))
                .collect();
            if on_true.is_empty() {
                return Narrowing::None;
            }
            Narrowing::Compound {
                base: Box::new(Narrowing::None),
                on_true,
                on_false: Vec::new(),
            }
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
        length_guard_var(compared_with_one(lhs, rhs)?)
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

/// A then-branch-only non-NULL fact; the else branch learns nothing.
fn non_null_then(var: String) -> Narrowing {
    Narrowing::Compound {
        base: Box::new(Narrowing::None),
        on_true: vec![Narrowing::Negative {
            var,
            target: RType::new(Mode::Null, Length::Zero),
        }],
        on_false: Vec::new(),
    }
}

/// The non-NULL facts a true conjunct proves: `!is.null(x)`, or the
/// facts of a nested conjunction or length guard.
fn non_null_then_facts(conjunct: Narrowing) -> Vec<Narrowing> {
    match conjunct {
        fact if is_non_null_fact(&fact) => vec![fact],
        Narrowing::Compound { base, on_true, .. }
            if matches!(*base, Narrowing::None)
                && !on_true.is_empty()
                && on_true.iter().all(is_non_null_fact) =>
        {
            on_true
        }
        _ => Vec::new(),
    }
}

/// Whether evaluating `expr` may bind `name`: an assignment to it (`<-`,
/// `<<-`, `assign("name", v)`) or a `for` over it at any depth outside
/// function bodies, including inside nested control flow.
fn may_assign(expr: &Expr, name: &str) -> bool {
    let binds = |target: &Expr| matches!(target, Expr::Ident { name: bound, .. } if bound == name);
    walk_expr(
        expr,
        Walk {
            fn_bodies: false,
            ..Walk::ALL
        },
        |node, _| match node {
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::Assign | BinOpKind::SuperAssign,
                lhs,
                ..
            }) if binds(lhs) => ControlFlow::Break(()),
            AstNode::Stmt(Stmt::Assign { target, .. }) if binds(target) => ControlFlow::Break(()),
            AstNode::Stmt(Stmt::For { name: bound, .. }) if bound == name => ControlFlow::Break(()),
            AstNode::Expr(Expr::Call { func, args, .. })
                if ident_name(func) == Some("assign")
                    && matches!(
                        args.first().map(|arg| &arg.value),
                        Some(Expr::String(bound, _)) if bound == name
                    ) =>
            {
                ControlFlow::Break(())
            }
            _ => ControlFlow::Continue(Descend::Into),
        },
    )
    .is_break()
}

/// The non-NULL facts a false disjunct proves: `is.null(x)`, or the facts
/// of a nested disjunction.
fn non_null_else_facts(disjunct: Narrowing) -> Vec<Narrowing> {
    match disjunct {
        Narrowing::Positive { var, target } if target.mode == Mode::Null => {
            vec![Narrowing::Negative { var, target }]
        }
        Narrowing::Compound {
            base,
            on_true,
            on_false,
        } if matches!(*base, Narrowing::None)
            && on_true.is_empty()
            && on_false.iter().all(is_non_null_fact) =>
        {
            on_false
        }
        _ => Vec::new(),
    }
}

fn is_non_null_fact(fact: &Narrowing) -> bool {
    matches!(fact, Narrowing::Negative { target, .. } if target.mode == Mode::Null)
}

/// The `x` of `length(x) == 1` or `length(x) > 0`: both are FALSE for NULL.
fn non_empty_length_guard_var(expr: &Expr) -> Option<String> {
    match expr {
        Expr::BinOp {
            op: BinOpKind::Eq,
            lhs,
            rhs,
            ..
        } => length_guard_var(compared_with_one(lhs, rhs)?),
        Expr::BinOp {
            op: BinOpKind::Gt,
            lhs,
            rhs,
            ..
        } if is_literal_eq(rhs, 0.0) => length_guard_var(lhs),
        _ => None,
    }
}

/// The argument of a `missing(x)` call, bare or with any `pkg::` prefix
/// (`bare_name`), as the supply analysis's `is_missing_head` reads it.
/// A qualified head is still an `Expr::Ident`, so `missing(x)` and
/// `!missing(x)` accept the same forms.
fn missing_test_var(expr: &Expr) -> Option<String> {
    let Expr::Call { func, args, .. } = expr else {
        return None;
    };
    let name = ident_name(func)?;
    if crate::semantic_lists::bare_name(name) != "missing" || args.len() != 1 {
        return None;
    }
    first_arg_ident(args)
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

/// For a comparison against the literal one written either way round
/// (`length(x) == 1`, `1L != length(x)`), the other operand.
pub(crate) fn compared_with_one<'a>(lhs: &'a Expr, rhs: &'a Expr) -> Option<&'a Expr> {
    if is_literal_eq(rhs, 1.0) {
        Some(lhs)
    } else if is_literal_eq(lhs, 1.0) {
        Some(rhs)
    } else {
        None
    }
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
        // Data frames are list-backed in the current type lattice.
        "is.data.frame" => {
            Some(RType::scalar(Mode::List).with_class(ClassVector::guard("data.frame")))
        }
        "is.null" => Some(RType::new(Mode::Null, Length::Zero)),
        "is.raw" => Some(RType::scalar(Mode::Raw)),
        _ => None,
    }
}

/// Whether `extract_builtin_type_narrowing` trusts the call head `name` by
/// spelling alone, so a rebinding or a non-`base::` qualifier must void it.
/// The `is.<class>` fallback, which project predicates use, is not.
fn builtin_guard_callee(name: &str) -> bool {
    let name = crate::semantic_lists::bare_name(name);
    predicate_target(name).is_some() || matches!(name, "inherits" | "length" | "missing")
}

pub(crate) fn s3_predicate_target(name: &str) -> Option<RType> {
    let class = name.strip_prefix("is.")?;
    if class.is_empty() {
        return None;
    }
    Some(RType::unknown().with_class(ClassVector::guard(class)))
}

/// Mark a stub-declared predicate or assertion target's classes as guard
/// evidence, like the built-in class tests. Union members, columns, and a
/// closure's return type carry their own classes.
pub(crate) fn guard_target(mut target: RType) -> RType {
    target.class.guarded = target.class.has_known_class();
    if let Some(sig) = &target.fn_sig {
        let mut sig = FunctionSignature::clone(sig);
        sig.return_type = Box::new(guard_target(*sig.return_type));
        target.fn_sig = Some(Arc::new(sig));
    }
    if let Some(members) = &target.members {
        target.members = Some(members.iter().cloned().map(guard_target).collect());
    }
    if let Some(schema) = &target.columns {
        let mut schema = ColumnSchema::clone(schema);
        for (_, column) in &mut schema.columns {
            *column = guard_target(column.clone());
        }
        target.columns = Some(Arc::new(schema));
    }
    target
}

/// A method selected from guard evidence need not be the one R dispatches
/// to, so the classes of its result are guard evidence as well.
pub(crate) fn dispatch_result(guarded_receiver: bool, result: RType) -> RType {
    if guarded_receiver {
        guard_target(result)
    } else {
        result
    }
}

/// The type a passing guard installs. A guard filters paths without
/// changing the value, so an exact known class outlives the guard's class
/// evidence. A default parameter's type describes only the omitted call.
/// A NULL cannot pass a non-NULL guard, so its zero length says nothing
/// about the value that does.
fn guarded_type(scope: &Scope, var: &str, existing: &RType, target: &RType) -> RType {
    let length = if existing.mode == Mode::Null && target.mode != Mode::Null {
        Length::Unknown
    } else {
        existing.length
    };
    let mut narrowed = RType {
        mode: target.mode,
        length,
        ..target.clone()
    };
    if existing.class.has_known_class()
        && !existing.class.guarded
        && !scope.is_default_parameter(var)
    {
        narrowed.class = existing.class.clone();
    }
    narrowed
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

/// Install the selected branch's failed-namespace fact (RY123): the
/// `FALSE` branch gains the package, the `TRUE` branch loses it (a nested
/// positive guard proves the namespace loaded).
pub(crate) fn apply_failed_namespace_branch(
    scope: &mut Scope,
    guard: &Option<(String, bool)>,
    branch: NarrowingBranch,
) {
    let Some((package, failed_then)) = guard else {
        return;
    };
    let failed = matches!(
        (branch, failed_then),
        (NarrowingBranch::Then, true) | (NarrowingBranch::Else, false)
    );
    if failed {
        scope.failed_namespaces.insert(package.clone());
    } else {
        scope.failed_namespaces.remove(package);
    }
}

/// The if-join for failed-namespace facts (RY123): a fact reaches the
/// continuation only if every continuing arm still holds it — an
/// arm-local clear (a call that may have loaded the namespace) is not
/// undone, and an arm-local install escapes only when its sibling
/// diverges. Shared by the journal and cloned-scope walkers so their
/// joins cannot drift.
pub(crate) fn join_failed_namespaces(
    base: &crate::FxSet<String>,
    then_set: &crate::FxSet<String>,
    else_set: &crate::FxSet<String>,
    then_diverges: bool,
    else_diverges: bool,
) -> crate::FxSet<String> {
    if base.is_empty() && then_set.is_empty() && else_set.is_empty() {
        return crate::FxSet::default();
    }
    base.iter()
        .chain(then_set.iter())
        .chain(else_set.iter())
        .filter(|package| {
            (then_diverges || then_set.contains(*package))
                && (else_diverges || else_set.contains(*package))
        })
        .cloned()
        .collect()
}

/// Apply all facts proved by the selected branch and return their binding names.
pub(crate) fn apply_narrowing_branch<'a>(
    scope: &mut Scope,
    narrowing: &'a Narrowing,
    branch: NarrowingBranch,
) -> Vec<&'a str> {
    if let Narrowing::Compound {
        base,
        on_true,
        on_false,
    } = narrowing
    {
        let mut names = apply_narrowing_branch(scope, base, branch);
        let facts = match branch {
            NarrowingBranch::Then => on_true,
            NarrowingBranch::Else => on_false,
        };
        for fact in facts {
            names.extend(apply_narrowing_branch(scope, fact, NarrowingBranch::Then));
        }
        names
    } else {
        apply_single_narrowing_branch(scope, narrowing, branch)
            .into_iter()
            .collect()
    }
}

/// Apply only the selected path's refinement. Assertions keep this scope;
/// conditional expressions supply a single isolated clone for their RHS.
fn apply_single_narrowing_branch<'a>(
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
                if disjoint_mode_guard(scope, var, &existing, target) {
                    scope.insert_narrowed(var.clone(), RType::unknown());
                    return Some(var.as_str());
                }
                let class_narrowing = target.class.has_known_class();
                let incompatible_parameter_default =
                    scope.is_default_parameter(var) && !types_intersect(&existing, target);
                let should_install = incompatible_parameter_default
                    || class_narrowing
                    || match existing.mode {
                        mode if mode.is_unknown_or_non_vector() => true,
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
                        || existing.mode.is_unknown_or_non_vector()
                        || matches!(existing.mode, Mode::Null | Mode::Union))
                {
                    let narrowed = guarded_type(scope, var, &existing, target);
                    scope.insert_narrowed(var.clone(), narrowed);
                    return Some(var.as_str());
                }
                // `is.null(x)` leaves a NULL default's type unchanged but
                // proves it on this path, so a later `Supplied` keeps it
                // (see the `Supplied` arm's narrowed-binding guard).
                if target.mode == Mode::Null
                    && existing.mode == Mode::Null
                    && scope.is_default_parameter(var)
                {
                    scope.insert_narrowed(var.clone(), existing);
                }
            }
        }
        (Narrowing::Positive { var, target }, NarrowingBranch::Else)
        | (Narrowing::Negative { var, target }, NarrowingBranch::Then) => {
            // Only NULL has a representable negative-path complement.
            if target.mode == Mode::Null && narrow_away_from_null_in(scope, var) {
                return Some(var.as_str());
            }
            if let Some(existing) = scope.get(var)
                && proven_class_guard(scope, var, existing, target)
            {
                scope.insert_narrowed(var.clone(), RType::unknown());
                return Some(var.as_str());
            }
            // When a mode predicate's false path proves the runtime value
            // is not any tested mode, the recorded type cannot describe it
            // here; degrade to unknown in this branch only. For a defaulted
            // parameter that type is only the omitted-argument call shape
            // (`diagnostic_parameter_type`); for any other binding R never
            // enters the branch, so code there cannot be proven to fail, the
            // mirror of `disjoint_mode_guard`. A known class on the value or
            // any union member keeps its type: `is.numeric()` is FALSE for a
            // Date or factor. A class predicate's false path rejects the
            // class claim, not the mode, so it never degrades, and mixed
            // unions keep any surviving member.
            if is_mode_predicate(target)
                && let Some(existing) = scope.get(var)
                && !existing.class.has_known_class()
                && existing.members.as_ref().is_none_or(|members| {
                    members.iter().all(|member| !member.class.has_known_class())
                })
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
        (Narrowing::ScalarElse { var, target }, NarrowingBranch::Else) => {
            if let Some(existing) = scope.get(var).cloned() {
                // A concrete NULL local cannot satisfy length(x) == 1. A
                // parameter default is not exhaustive: callers can pass a scalar.
                if existing.mode == Mode::Null && !scope.is_default_parameter(var) {
                    scope.unreachable = true;
                } else {
                    let mut scalar = match target {
                        Some(target) => guarded_type(scope, var, &existing, target),
                        None if existing.mode == Mode::Null => RType::unknown(),
                        None => existing,
                    };
                    scalar.length = Length::One;
                    scope.insert_narrowed(var.clone(), scalar);
                    return Some(var.as_str());
                }
            }
        }
        (Narrowing::Unreached { var }, NarrowingBranch::Then) => {
            scope.insert_narrowed(var.clone(), RType::unknown());
            return Some(var.as_str());
        }
        (Narrowing::ScalarThen { var }, NarrowingBranch::Then) => {
            scope.clear_loop_vector(var);
        }
        // Only the omitted-call type goes; an earlier guard's fact stays.
        // The `is.null(x)` NULL-default proof in the Positive arm above
        // relies on this: it marks `x` narrowed so that a nested
        // `if (!missing(x))` keeps the NULL for `f(NULL)`.
        (Narrowing::Supplied { var }, NarrowingBranch::Then)
            if scope.is_default_parameter(var) && !scope.narrowed_bindings.contains(var) =>
        {
            scope.insert_narrowed(var.clone(), RType::unknown());
            return Some(var.as_str());
        }
        _ => {}
    }
    None
}

/// The target of a predicate a compound guard accepts: a one-argument
/// type predicate, or `inherits(x, "cls")` with a literal class.
fn compound_predicate_target(name: &str, args: &[Arg]) -> Option<RType> {
    match crate::semantic_lists::bare_name(name) {
        "inherits" if args.len() == 2 => predicate_call_target("inherits", args),
        name if args.len() == 1 => predicate_target(name),
        _ => None,
    }
}

/// Whether `target` tests only a mode (`is.list`, `is.numeric`, ...).
/// `Mode::Opaque` targets are class predicates (`inherits`, `is.<class>`),
/// and a concrete mode can still carry a class claim (`is.data.frame` is
/// list + "data.frame").
fn is_mode_predicate(target: &RType) -> bool {
    matches!(
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
    ) && !target.class.has_known_class()
}

/// A passing mode guard on a binding ry typed with no member of the tested
/// mode, other than a default parameter (which installs the tested type).
/// Either the branch is dead or a caller passed what ry's type misses. The
/// tested type would make dead branches report errors on it (`dof + 1`
/// under `is.character(dof)`), so the branch sees an unknown value instead
/// (#741, #752).
fn disjoint_mode_guard(scope: &Scope, var: &str, existing: &RType, target: &RType) -> bool {
    is_mode_predicate(target)
        && !existing.mode.is_unknown_or_non_vector()
        && existing.mode != Mode::Null
        && !scope.is_default_parameter(var)
        && !types_intersect(existing, target)
}

/// Base `is.numeric.Date`, `is.numeric.POSIXt` and `is.numeric.difftime`
/// return FALSE, and so does the primitive for a factor. Any other class
/// keeps `is.numeric()` TRUE for an integer or double. Whether every value
/// of `existing` carries one of these classes.
fn never_numeric(existing: &RType) -> bool {
    let rejected = |cv: &ClassVector| {
        ["Date", "POSIXt", "difftime", "factor"]
            .iter()
            .any(|class| cv.contains(class))
    };
    rejected(&existing.class)
        || existing.members.as_ref().is_some_and(|members| {
            !members.is_empty() && members.iter().all(|member| rejected(&member.class))
        })
}

/// A failing class test (`!inherits(x, "Date")`) on a value whose fully known
/// classes include every tested class: R never enters that branch.
fn proven_class_guard(scope: &Scope, var: &str, existing: &RType, target: &RType) -> bool {
    target.class.has_known_class()
        && existing.class.has_known_class()
        && !existing.class.guarded
        && (existing.class.len as usize) < existing.class.names.len()
        && !scope.is_default_parameter(var)
        && target
            .class
            .names
            .iter()
            .take(target.class.len as usize)
            .all(|class| {
                class
                    .as_deref()
                    .is_some_and(|class| existing.class.contains(class))
            })
}

fn install_positive_narrowing(scope: &mut Scope, var: &str, target: &RType) -> bool {
    let Some(existing) = scope.get(var).cloned() else {
        return false;
    };
    if disjoint_mode_guard(scope, var, &existing, target) {
        scope.insert_narrowed(var.to_string(), RType::unknown());
        return true;
    }
    let class_narrowing = target.class.has_known_class();
    let incompatible_parameter_default =
        scope.is_default_parameter(var) && !types_intersect(&existing, target);
    let should_install = incompatible_parameter_default
        || class_narrowing
        || existing.mode.is_unknown_or_non_vector()
        || matches!(existing.mode, Mode::Null | Mode::Union);
    if should_install {
        let narrowed = guarded_type(scope, var, &existing, target);
        scope.insert_narrowed(var.to_string(), narrowed);
        return true;
    }
    false
}

impl Checker {
    /// `is.numeric(x)` or its negation, resolved to base, on a binding
    /// [`never_numeric`] covers: the binding and the truth of `cond` on the
    /// path where `is.numeric(x)` would be TRUE, which R never takes.
    fn never_numeric_test(&self, cond: &Expr, scope: &Scope) -> Option<(String, bool)> {
        let mut truth = true;
        let mut test = cond;
        while let Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } = test
        {
            truth = !truth;
            test = expr;
        }
        let Expr::Call { func, args, .. } = test else {
            return None;
        };
        let name = ident_name(func)?;
        let var = first_arg_ident(args)?;
        (crate::semantic_lists::bare_name(name) == "is.numeric"
            && args.len() == 1
            && self.resolves_to_base(name, scope)
            && scope.get(&var).is_some_and(|existing| {
                never_numeric(existing) && !self.s3_method_may_apply("is.numeric", existing, scope)
            }))
        .then_some((var, truth))
    }

    /// The subjects of the guards in `cond` whose built-in callee
    /// (`is.null(x)`, `length(x)`, ...) may be rebound here. A project or
    /// local redefinition may answer anything, so it proves nothing about
    /// its subject; the other guards in `cond` still do.
    fn rebound_guard_subjects(&self, cond: &Expr, scope: &Scope, subjects: &mut HashSet<String>) {
        match cond {
            Expr::UnaryOp { expr, .. } => self.rebound_guard_subjects(expr, scope, subjects),
            Expr::BinOp { lhs, rhs, .. } => {
                self.rebound_guard_subjects(lhs, scope, subjects);
                self.rebound_guard_subjects(rhs, scope, subjects);
            }
            Expr::Call { func, args, .. } => {
                if let Some(name) = ident_name(func)
                    && builtin_guard_callee(name)
                    && !self.guard_callee_resolves_to_base(name, scope)
                    && let Some(subject) = first_arg_ident(args)
                {
                    subjects.insert(subject);
                }
            }
            _ => {}
        }
    }

    /// Restrict compound facts to predicates without intervening writes or calls.
    fn compound_guard_is_pure(&self, cond: &Expr, scope: &Scope) -> bool {
        match cond {
            Expr::BinOp {
                op: BinOpKind::AndAnd | BinOpKind::OrOr,
                lhs,
                rhs,
                ..
            } => self.compound_guard_is_pure(lhs, scope) && self.compound_guard_is_pure(rhs, scope),
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                ..
            } => self.compound_guard_is_pure(expr, scope),
            Expr::Call { func, args, .. } => {
                let Some(name) = ident_name(func) else {
                    return false;
                };
                compound_predicate_target(name, args).is_some()
                    && self.resolves_to_base(name, scope)
                    && !scope.data_mask_unknown
                    && !scope.uncertainty.effects
                    && matches!(args.first().map(|arg| &arg.value), Some(Expr::Ident { .. }))
            }
            _ => false,
        }
    }

    fn compound_guard_facts(cond: &Expr, truth: bool) -> Vec<Narrowing> {
        match cond {
            Expr::BinOp { op, lhs, rhs, .. }
                if (*op == BinOpKind::AndAnd && truth) || (*op == BinOpKind::OrOr && !truth) =>
            {
                let mut facts = Self::compound_guard_facts(lhs, truth);
                facts.extend(Self::compound_guard_facts(rhs, truth));
                facts
            }
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                ..
            } => Self::compound_guard_facts(expr, !truth),
            Expr::Call { func, args, .. } => {
                let Some(target) =
                    ident_name(func).and_then(|name| compound_predicate_target(name, args))
                else {
                    return Vec::new();
                };
                let Some(var) = first_arg_ident(args) else {
                    return Vec::new();
                };
                vec![if truth {
                    Narrowing::Positive { var, target }
                } else {
                    Narrowing::Negative { var, target }
                }]
            }
            _ => Vec::new(),
        }
    }

    /// The facts `cond` proves, minus those about the subject of a guard
    /// whose built-in callee may be rebound.
    pub(crate) fn extract_type_narrowing(&self, cond: &Expr, scope: &Scope) -> Narrowing {
        let mut rebound = HashSet::new();
        self.rebound_guard_subjects(cond, scope, &mut rebound);
        let narrowing = self.extract_unscreened_type_narrowing(cond, scope);
        if rebound.is_empty() {
            narrowing
        } else {
            narrowing.without(&rebound)
        }
    }

    /// Signature-declared predicates extend the built-in predicate vocabulary
    /// only when ordinary typeshed resolution establishes their provenance.
    fn extract_unscreened_type_narrowing(&self, cond: &Expr, scope: &Scope) -> Narrowing {
        let built_in = extract_builtin_type_narrowing(cond);
        if let Some((var, truth)) = self.never_numeric_test(cond, scope) {
            let fact = vec![Narrowing::Unreached { var }];
            let (on_true, on_false) = if truth {
                (fact, Vec::new())
            } else {
                (Vec::new(), fact)
            };
            return Narrowing::Compound {
                base: Box::new(built_in),
                on_true,
                on_false,
            };
        }
        if !scope.loop_vector_bindings.is_empty()
            && let Some(var) = self.scalar_assertion_subject(cond, scope)
        {
            return Narrowing::Compound {
                base: Box::new(built_in),
                on_true: vec![Narrowing::ScalarThen { var }],
                on_false: Vec::new(),
            };
        }
        let mut compound = cond;
        while let Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } = compound
        {
            compound = expr;
        }
        if matches!(
            compound,
            Expr::BinOp {
                op: BinOpKind::AndAnd | BinOpKind::OrOr,
                ..
            }
        ) && self.compound_guard_is_pure(cond, scope)
        {
            return Narrowing::Compound {
                base: Box::new(built_in),
                on_true: Self::compound_guard_facts(cond, true),
                on_false: Self::compound_guard_facts(cond, false),
            };
        }
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
            target: guard_target(json_rtype_to_rtype(&predicate.target)),
        }
    }

    /// A literal-package `requireNamespace` test whose result selects a
    /// branch (RY123): the package and whether the `then` branch is the
    /// one where loading failed. Base `requireNamespace` returns `FALSE`
    /// on a failed load with or without `quietly = TRUE` (it wraps
    /// `loadNamespace` in `tryCatch`; `quietly` only suppresses the
    /// message), so no `quietly` shape is required. A masked
    /// `requireNamespace` (anything [`Checker::resolves_to_base`]
    /// rejects) and a dynamic package name prove nothing.
    pub(crate) fn failed_namespace_guard(
        &self,
        cond: &Expr,
        scope: &Scope,
    ) -> Option<(String, bool)> {
        let mut negated = false;
        let mut test = cond;
        while let Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } = test
        {
            if ops_chooser::operator_rebound(self, "!", scope) {
                return None;
            }
            negated = !negated;
            test = expr;
        }
        let Expr::Call { func, args, .. } = test else {
            return None;
        };
        let name = ident_name(func)?;
        if crate::semantic_lists::bare_name(name) != "requireNamespace"
            || !self.resolves_to_base(name, scope)
            || scope.data_mask_unknown
        {
            return None;
        }
        let mut named_package = args
            .iter()
            .filter(|arg| arg.name.as_deref() == Some("package"));
        let named_package = [named_package.next(), named_package.next()];
        let package_arg = match named_package {
            [Some(arg), None] => {
                // A named `package` plus an unnamed first argument is a
                // duplicate match in R, not a guard.
                if args.first().is_some_and(|first| first.name.is_none()) {
                    return None;
                }
                arg
            }
            [None, None] => args.first().filter(|first| first.name.is_none())?,
            _ => return None,
        };
        let Expr::String(package, _) = &package_arg.value else {
            return None;
        };
        if package.is_empty() {
            return None;
        }
        Some((package.clone(), negated))
    }

    /// The shared RY123 access gate for value and call-head positions:
    /// the cheap empty-fact check runs first (the set is empty in
    /// almost all inference), then the qualified spelling must not be
    /// bound locally under either position's rule.
    pub(crate) fn maybe_check_failed_namespace_access(
        &mut self,
        name: &str,
        span: Span,
        scope: &Scope,
    ) {
        if scope.failed_namespaces.is_empty()
            || !name.contains("::")
            || super::call::is_user_infix_name(name)
            || self.qualified_name_bound_locally(name, scope)
        {
            return;
        }
        self.check_failed_namespace_access(name, span, scope);
    }

    /// RY123: a `pkg::name` / `pkg:::name` access whose package is proven
    /// unloaded on this branch. A rebound `::`/`:::` operator runs instead
    /// of the namespace lookup, so it proves nothing.
    pub(crate) fn check_failed_namespace_access(&mut self, name: &str, span: Span, scope: &Scope) {
        if scope.failed_namespaces.is_empty() || self.caught_namespace_probe_depth > 0 {
            return;
        }
        // A fully backticked `` `pkg::f` `` is one lexical symbol, not
        // an access; the parser keeps its ticks (a real access arrives
        // decoded, e.g. `` `my::pkg`::f `` as `my::pkg::f`, which
        // `split_qualified` keys on the literal `my::pkg`).
        if name.starts_with('`') && name.ends_with('`') && name.len() >= 2 {
            return;
        }
        let Some((package, _)) = crate::resolve::split_qualified(name) else {
            return;
        };
        if !scope.failed_namespaces.contains(package) {
            return;
        }
        // The operator is the split `split_qualified` made: `:::`
        // only when the raw package part ends in a third colon, so a
        // quoted package containing `:::` (`` `a:::b`::c ``) still
        // keys the rebound test on `::`.
        let operator = match name.rsplit_once("::") {
            Some((pkg_raw, _)) if pkg_raw.ends_with(':') => ":::",
            _ => "::",
        };
        if ops_chooser::operator_rebound(self, operator, scope) {
            return;
        }
        self.emit(
            Severity::Warning,
            span,
            "RY123",
            format!(
                "`{name}` needs the `{package}` namespace, but `requireNamespace(\"{package}\")` returned `FALSE` on this branch"
            ),
        );
    }

    /// Infer a caught `tryCatch` probe expression with RY123 silenced:
    /// the depth inc/dec pairing lives in this one small function so
    /// early returns inside inference cannot unbalance it.
    pub(crate) fn infer_caught_namespace_probe(&mut self, expr: &Expr, scope: &mut Scope) -> RType {
        self.caught_namespace_probe_depth += 1;
        let result = self.infer(expr, scope);
        self.caught_namespace_probe_depth -= 1;
        result
    }

    /// The narrowing for a condition that selects code: an `if` test, the
    /// left of `&&`/`||`, or an assertion (`stopifnot()`) whose passing path
    /// is the continuation. A defaulted parameter's type describes only the
    /// omitted-argument call shape (`diagnostic_parameter_type`). A passing
    /// test that reads it but proves nothing about it (`is_tensor(dims)`,
    /// `"pt" %in% colnames(width)`) may be exactly the test that default
    /// fails, so the arm it selects sees what a supplied argument would
    /// (#710).
    pub(crate) fn extract_branch_narrowing(&self, cond: &Expr, scope: &Scope) -> Narrowing {
        // `if (has_x)` / `if (!has_x)` test the predicate `has_x` holds.
        let negated;
        let cond = match cond {
            Expr::Ident { name, .. } => scope.condition_alias(name).unwrap_or(cond),
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                span,
            } => match ident_name(expr).and_then(|name| scope.condition_alias(name)) {
                Some(alias) => {
                    negated = Expr::UnaryOp {
                        op: UnaryOpKind::Not,
                        expr: Box::new(alias.clone()),
                        span: *span,
                    };
                    &negated
                }
                None => cond,
            },
            _ => cond,
        };
        let narrowing = self.extract_type_narrowing(cond, scope);
        if scope.default_parameter_bindings.is_empty() {
            return narrowing;
        }
        let undecided = |truth: bool| -> Vec<Narrowing> {
            self.tested_names(cond, truth, scope)
                .into_iter()
                .filter(|name| {
                    scope.is_default_parameter(name)
                        && scope.get(name).is_some_and(|t| !t.mode.is_unknown())
                        && !narrowing.mentions(name)
                })
                .map(|var| Narrowing::Supplied { var })
                .collect()
        };
        let (on_true, on_false) = (undecided(true), undecided(false));
        if on_true.is_empty() && on_false.is_empty() {
            return narrowing;
        }
        Narrowing::Compound {
            base: Box::new(narrowing),
            on_true,
            on_false,
        }
    }

    /// The names that some passing (TRUE) test ry cannot decide read, on
    /// every way `cond` can evaluate to `truth`. A failing test proves
    /// nothing, and neither does a test ry narrows itself (`missing(x)`,
    /// `is.null(x)`). `a && b` is TRUE one way (both TRUE) and FALSE two
    /// ways (`a` FALSE, or `a` TRUE then `b` FALSE), so the names of a
    /// two-way path are those both ways share; `||` is the mirror image and
    /// `!` flips the path.
    fn tested_names(&self, cond: &Expr, truth: bool, scope: &Scope) -> BTreeSet<String> {
        match cond {
            Expr::BinOp {
                op: op @ (BinOpKind::And | BinOpKind::AndAnd | BinOpKind::Or | BinOpKind::OrOr),
                lhs,
                rhs,
                ..
            } => {
                let both = self.tested_names(rhs, truth, scope);
                let lhs_decides = self.tested_names(lhs, truth, scope);
                if matches!(op, BinOpKind::And | BinOpKind::AndAnd) == truth {
                    lhs_decides.union(&both).cloned().collect()
                } else {
                    let mut rhs_decides = self.tested_names(lhs, !truth, scope);
                    rhs_decides.extend(both);
                    lhs_decides.intersection(&rhs_decides).cloned().collect()
                }
            }
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                ..
            } => self.tested_names(expr, !truth, scope),
            _ if truth => {
                let mut names = BTreeSet::new();
                let _ = self.visit_evaluated_names(cond, UnseenReads::Read, &mut |name| {
                    names.insert(name.to_owned());
                    ControlFlow::Continue(())
                });
                let decided = self.extract_type_narrowing(cond, scope);
                names.retain(|name| !decided.mentions(name));
                names
            }
            _ => BTreeSet::new(),
        }
    }

    /// Calls `on_read` with each name `expr` evaluates, until it breaks.
    /// Field names, assignment targets, closures, formulas, and arguments a
    /// stub quotes, captures, or evaluates in a data mask (`quote(x)`,
    /// `substitute(x)`, `filter(df, x > 1)`, where `x` may be a column) are
    /// not reads. `unseen` decides the calls ry cannot see into.
    pub(crate) fn visit_evaluated_names(
        &self,
        expr: &Expr,
        unseen: UnseenReads,
        on_read: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        ry_core::stack::grow(|| {
            let walk = Walk {
                assign_targets: false,
                assign_operands: false,
                dollar_args: false,
                fn_bodies: false,
                ..Walk::ALL
            };
            let unseen_call = || match unseen {
                UnseenReads::Read => ControlFlow::Continue(Descend::Into),
                UnseenReads::Skip => ControlFlow::Continue(Descend::Skip),
            };
            walk_expr(expr, walk, |node, _| match node {
                AstNode::Expr(Expr::Ident { name, .. }) => {
                    on_read(name)?;
                    ControlFlow::Continue(Descend::Into)
                }
                AstNode::Expr(Expr::Call { func, args, .. }) => {
                    let Some(callee) = ident_name(func) else {
                        return unseen_call();
                    };
                    if crate::semantic_lists::is_quoting_form(callee) {
                        return ControlFlow::Continue(Descend::Skip);
                    }
                    if unseen == UnseenReads::Skip && crate::collect::probes_promise(func) {
                        return ControlFlow::Continue(Descend::Skip);
                    }
                    let Some(signature) = self.resolve_typeshed_sig_ref(callee) else {
                        return unseen_call();
                    };
                    let bindings = match_params(&signature.params, args);
                    for (index, argument) in args.iter().enumerate() {
                        if !matches!(
                            eval_mode_for_arg(signature, &bindings, index),
                            Some(
                                EvalMode::QuotedSymbol
                                    | EvalMode::QuotedExpression
                                    | EvalMode::CapturesPromise
                                    | EvalMode::DataMask
                                    | EvalMode::TidySelect
                            )
                        ) {
                            self.visit_evaluated_names(&argument.value, unseen, on_read)?;
                        }
                    }
                    ControlFlow::Continue(Descend::Skip)
                }
                _ => ControlFlow::Continue(Descend::Into),
            })
        })
    }
}

/// How [`Checker::visit_evaluated_names`] treats what ry cannot see into:
/// a call no stub resolves (or whose callee is computed) and `missing(x)`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnseenReads {
    /// Such a call reads its callee and arguments, and `missing(x)` reads
    /// `x`: the branch analysis counts every possible read, so a test it
    /// cannot decide drops a default's omitted-call type (#710).
    Read,
    /// Such a call is opaque, and `missing(x)` probes without reading:
    /// a default that may force a parameter errs toward silence (#790).
    Skip,
}

#[cfg(test)]
mod selected_branch_tests {
    use super::*;

    #[test]
    fn an_unnamed_tested_class_does_not_prove_the_class_test() {
        let scope = Scope::default();
        let date = RType::scalar(Mode::Double).with_class(ClassVector::single("Date"));
        let target = |class: ClassVector| RType::unknown().with_class(class);
        assert!(proven_class_guard(
            &scope,
            "x",
            &date,
            &target(ClassVector::single("Date"))
        ));
        let partial = ClassVector {
            names: [Some(Arc::from("Date")), None, None, None],
            len: 2,
            known: true,
            guarded: false,
        };
        assert!(!proven_class_guard(&scope, "x", &date, &target(partial)));
    }

    #[test]
    fn class_carrying_predicate_targets_do_not_degrade_the_rejected_branch() {
        // `is.data.frame` narrows to list + "data.frame": its false path
        // rejects the class claim, not the list mode, so the recorded type
        // must survive the rejected branch untouched even when the mode
        // alone would match.
        let mut scope = Scope::default();
        scope.insert_parameter_default("x", RType::scalar(Mode::List));
        apply_single_narrowing_branch(
            &mut scope,
            &Narrowing::Positive {
                var: "x".into(),
                target: RType::scalar(Mode::List).with_class(ClassVector::single("data.frame")),
            },
            NarrowingBranch::Else,
        );
        assert_eq!(
            scope.get("x"),
            Some(&RType::scalar(Mode::List)),
            "a classed predicate must not degrade the default-parameter type"
        );
    }

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
    fn positive_and_negated_guards_on_a_disjoint_union_give_unknown() {
        let original = RType::union(Arc::from([
            RType::scalar(Mode::Integer),
            RType::scalar(Mode::Character),
        ]));
        let mut positive = Scope::default();
        positive.insert("x", original.clone());
        let mut negative = positive.clone();
        let target = RType::scalar(Mode::Double);
        // No member is a double, so neither passing path keeps the stale
        // union or installs the tested type (#752).
        assert_eq!(
            apply_single_narrowing_branch(
                &mut positive,
                &Narrowing::Positive {
                    var: "x".into(),
                    target: target.clone(),
                },
                NarrowingBranch::Then,
            ),
            Some("x")
        );
        assert_eq!(positive.get("x"), Some(&RType::unknown()));
        assert_eq!(
            apply_single_narrowing_branch(
                &mut negative,
                &Narrowing::Negative {
                    var: "x".into(),
                    target: target.clone(),
                },
                NarrowingBranch::Else,
            ),
            Some("x")
        );
        assert_eq!(negative.get("x"), Some(&RType::unknown()));
    }

    #[test]
    fn in_place_narrowing_preserves_parameter_and_scope_metadata() {
        let mut scope = Scope::default().with_unknown_data_mask();
        scope.mark_search_path_unknown();
        scope.insert_parameter_default("x", RType::scalar(Mode::Null));
        scope.mark_list_origin("x");
        scope.set_function_alias("x", "base::identity".into());
        scope.mark_lexical_function("x", Span::default());
        scope.insert_parameter("untouched", RType::scalar(Mode::Integer));
        scope.set_function_alias("untouched", "other".into());
        scope.mark_lexical_function("untouched", Span::default());
        scope.tidy_injection = Some(InjectionMode::Full);
        apply_single_narrowing_branch(
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
                apply_single_narrowing_branch(&mut scope, &narrowing, NarrowingBranch::Then),
                None
            );
            assert!(!scope.unreachable);
            let changed =
                apply_single_narrowing_branch(&mut scope, &narrowing, NarrowingBranch::Else);
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
