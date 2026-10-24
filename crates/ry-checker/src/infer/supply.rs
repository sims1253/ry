//! Supplied-vs-defaulted state for function formals, and RY108
//! (seq-defaulted-forward).
//!
//! A defaulted formal has two runtime identities: *supplied* (the caller
//! passed an argument) and *defaulted* (evaluation falls back to the
//! default expression). `missing(p)` tells them apart. It stays correct
//! after the promise is forced and becomes `FALSE` only once the name is
//! reassigned or used as a loop variable. `p <<- v` leaves it intact
//! (R 4.6.1). Forcing the formal does not choose an identity, so a
//! defaulted `to` read inside a method looks like a supplied one.
//!
//! The analysis walks one formal's body:
//!
//! * [`SupplyPoint::alive`] is `None` when every path rebound the name,
//!   otherwise the [`Supply`] fact for paths where the promise is still
//!   readable. A read reports when that fact is untested or proven
//!   defaulted, so `if (missing(p)) seq(x, p, ...)` fires. A path that
//!   rebound the name contributes no live promise to a merge, which keeps
//!   `if (missing(p)) p <- fallback` from poisoning the fall-through.
//! * A `missing(p)` test at an `if` refines like a type narrowing. A
//!   diverging arm (`return`, `stop()`, `UseMethod`) contributes nothing to
//!   the continuation. `&&`, `||`, and `!` decode positionally. Cached
//!   tests (`m <- missing(p)`, `seq.Date`'s `mTo`) decode until reassigned.
//!   A test the walker cannot decode still shows the author's awareness
//!   and silences the formal.
//! * `p <- value` walks the value, then kills the promise.
//! * `for (p in ...)` kills the promise even on a zero-trip loop:
//!   R assigns the loop variable anyway.
//!
//! Known gaps that stay quiet: reads inside nested closures, `break` paths
//! out of `while`, and reads in a supply test's own condition
//! (`p > 0 || missing(p)`). A loop whose iterator may be empty is still
//! walked for reads, but its exit state is discarded, since a guard in
//! its body may never run. Known gaps that
//! over-fire on some runtimes: a guard inside a loop under an unknown
//! `while` condition, and the else-fact of `A && missing(p)` when `A` is
//! unknown.
//!
//! RY108 checks the seq generic's `to`. A method that forwards a
//! defaulted `to` makes `seq(x, length.out = 3)` reach `seq.default` with
//! `to` as a supplied endpoint, which returns repeated values
//! (`seq(1, 1, length.out = 3)` is `1 1 1`). `from` is not analyzed
//! because `seq.default` consumes it on every path. `by` and `length.out`
//! are not analyzed because a forwarded default for them makes R raise
//! "too many arguments" itself.

use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr};
use std::ops::ControlFlow;

/// What a `missing()` view proves about one formal's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Supply {
    /// No test dominates: the value may still be the default.
    Untested,
    /// The caller supplied the argument. Reads are safe.
    Supplied,
    /// The caller omitted the argument, so a read forwards the default.
    Defaulted,
}

impl Supply {
    /// The fact that holds on both inputs. Unequal facts meet at
    /// `Untested`.
    fn merge(self, other: Self) -> Self {
        if self == other { self } else { Self::Untested }
    }

    /// The more specific of two facts. Used where both operands of `&&`
    /// must hold, so either proof settles the arm.
    fn refine(self, other: Self) -> Self {
        if self == Self::Untested { other } else { self }
    }

    /// Whether a read at this supply can forward a defaulted value.
    fn may_read_default(self) -> bool {
        !matches!(self, Self::Supplied)
    }
}

/// The per-point state of one formal. `alive == None` means every path
/// reaching the point rebound the name.
#[derive(Clone, Debug)]
struct SupplyPoint {
    alive: Option<Supply>,
    /// Cached tests `m <- missing(p)` / `m <- !missing(p)`: name -> whether
    /// the binding holds the *negation* of the test.
    proxies: HashMap<String, bool>,
}

impl SupplyPoint {
    fn new() -> Self {
        Self {
            alive: Some(Supply::Untested),
            proxies: HashMap::new(),
        }
    }
}

/// Outcome of decomposing one condition subtree.
enum AtomOutcome {
    /// A decoded `missing()` test (direct or cached): the then-arm and
    /// else-arm facts.
    Test { then: Supply, else_: Supply },
    /// The subtree contains no supply test for this formal.
    NoInfo,
    /// A supply test in a shape the walker cannot decode
    /// (`isTRUE(missing(p))`, a comparison). The author showed awareness,
    /// so silence is the safe reading.
    Mention,
}

impl AtomOutcome {
    /// The (then, else) supply facts. `NoInfo` is the untested pair.
    fn facts(self) -> (Supply, Supply) {
        match self {
            AtomOutcome::Test { then, else_ } => (then, else_),
            AtomOutcome::NoInfo | AtomOutcome::Mention => (Supply::Untested, Supply::Untested),
        }
    }

    /// `a && b`: either proof settles the then-arm. The else-arm is
    /// `!a || !b`, where one disjunct's fact does not survive the other
    /// being unknown.
    fn combine_and(self, rhs: Self) -> Self {
        match (self, rhs) {
            (AtomOutcome::Mention, _) | (_, AtomOutcome::Mention) => AtomOutcome::Mention,
            (AtomOutcome::NoInfo, AtomOutcome::NoInfo) => AtomOutcome::NoInfo,
            (a, b) => {
                let (then_a, else_a) = a.facts();
                let (then_b, else_b) = b.facts();
                AtomOutcome::Test {
                    then: then_a.refine(then_b),
                    else_: else_a.merge(else_b),
                }
            }
        }
    }

    /// `a || b`: dual of `&&`.
    fn combine_or(self, rhs: Self) -> Self {
        match (self, rhs) {
            (AtomOutcome::Mention, _) | (_, AtomOutcome::Mention) => AtomOutcome::Mention,
            (AtomOutcome::NoInfo, AtomOutcome::NoInfo) => AtomOutcome::NoInfo,
            (a, b) => {
                let (then_a, else_a) = a.facts();
                let (then_b, else_b) = b.facts();
                AtomOutcome::Test {
                    then: then_a.merge(then_b),
                    else_: else_a.refine(else_b),
                }
            }
        }
    }

    /// The facts with the arms exchanged. Swapping under negation applies
    /// De Morgan: the true path of `!(A || B)` is `!A && !B`.
    fn swapped(self) -> Self {
        match self {
            AtomOutcome::Test { then, else_ } => AtomOutcome::Test {
                then: else_,
                else_: then,
            },
            other => other,
        }
    }
}

/// The first read of `formal` in `body` that may evaluate an untested or
/// proven-defaulted value. `None` means every use is guarded by a
/// `missing()` test, reads a rebinding, or is defused (`substitute(p)`).
/// Records no diagnostics and leaves scope alone.
pub(crate) fn first_unguarded_defaulted_use(
    checker: &Checker,
    body: &[Stmt],
    formal: &str,
) -> Option<Span> {
    let mut walk = FormalSupplyWalk {
        checker,
        formal,
        hit: None,
    };
    let mut point = SupplyPoint::new();
    walk.walk_stmts(body, &mut point);
    walk.hit
}

struct FormalSupplyWalk<'c, 'a> {
    checker: &'c Checker,
    formal: &'a str,
    hit: Option<Span>,
}

impl FormalSupplyWalk<'_, '_> {
    fn walk_stmts(&mut self, stmts: &[Stmt], point: &mut SupplyPoint) {
        if self.hit.is_some() {
            return;
        }
        for statement in stmts {
            self.walk_stmt(statement, point);
            if self.hit.is_some() {
                return;
            }
        }
    }

    fn walk_stmt(&mut self, statement: &Stmt, point: &mut SupplyPoint) {
        ry_core::stack::grow(|| self.walk_stmt_inner(statement, point))
    }

    fn walk_stmt_inner(&mut self, statement: &Stmt, point: &mut SupplyPoint) {
        match statement {
            Stmt::Assign { target, value, .. } => {
                // The statement `p <<- v` lowers to Assign with a
                // SuperAssign wrapper around the value. R assigns in an
                // enclosing frame, so the local promise survives. The
                // wrapper's left operand restates the target.
                if let Expr::BinOp {
                    op: BinOpKind::SuperAssign,
                    rhs,
                    ..
                } = value
                {
                    self.walk_expr(rhs, point);
                    return;
                }
                self.walk_assign(target, value, point);
            }
            Stmt::Expr(expression) => self.walk_expr(expression, point),
            Stmt::If {
                cond, then, else_, ..
            } => self.walk_if(cond, Some(then), else_.as_deref(), point),
            Stmt::For {
                name, iter, body, ..
            } => {
                self.walk_expr(iter, point);
                let shadows = name == self.formal;
                let mut inner = point.clone();
                if shadows {
                    // Reads inside the body see the loop variable.
                    inner.alive = None;
                }
                self.walk_stmts(body, &mut inner);
                if shadows {
                    // R assigns the loop variable even on a zero-trip
                    // loop (`for (p in NULL)` leaves `missing(p)` FALSE).
                    point.alive = None;
                } else if iterator_provably_nonempty(iter) {
                    // The body runs at least once, so its first-iteration
                    // exit approximates the post-loop state (a `break`
                    // before the last statement included).
                    point.alive = inner.alive;
                    point.proxies = inner.proxies;
                }
            }
            Stmt::While { cond, body, .. } => {
                let (then_alive, else_alive) = self.condition_arms(cond, point);
                let mut inner = point.clone();
                inner.alive = then_alive;
                self.walk_stmts(body, &mut inner);
                // `repeat body` lowers to While with a constant-true
                // condition: the body runs at least once, so its exit
                // approximates the post-loop state. Otherwise leaving the
                // loop requires the condition to fail. Both ignore
                // `break`-mid-body paths.
                if matches!(cond, Expr::Logical(true, _)) {
                    point.alive = inner.alive;
                    point.proxies = inner.proxies;
                } else {
                    point.alive = else_alive;
                }
            }
            // A nested closure's body and parameter defaults evaluate when
            // the closure is called, not here, so the definition-site
            // supply state would misclassify both directions. Reads only
            // inside closures stay silent.
            Stmt::FunctionDef { .. } => {}
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    self.walk_expr(value, point);
                }
            }
        }
    }

    /// `target <- value`: R evaluates the value first, then rebinds. A plain
    /// name target is not a read. A complex target is read and replaces
    /// the promise.
    fn walk_assign(&mut self, target: &Expr, value: &Expr, point: &mut SupplyPoint) {
        self.walk_expr(value, point);
        match target {
            Expr::Ident { name, .. } => {
                if name == self.formal {
                    point.alive = None;
                } else {
                    // Any other binding drops a cached test.
                    let cached = match value {
                        Expr::UnaryOp {
                            op: UnaryOpKind::Not,
                            expr,
                            ..
                        } => self.is_missing_test(expr).then_some(true),
                        expr => self.is_missing_test(expr).then_some(false),
                    };
                    match cached {
                        Some(negated) => {
                            point.proxies.insert(name.to_string(), negated);
                        }
                        None => {
                            point.proxies.remove(name);
                        }
                    }
                }
            }
            target => {
                self.walk_expr(target, point);
                if self.references_formal(target) {
                    point.alive = None;
                }
            }
        }
    }

    fn walk_if(
        &mut self,
        cond: &Expr,
        then: Option<&[Stmt]>,
        else_: Option<&[Stmt]>,
        point: &mut SupplyPoint,
    ) {
        let (then_alive, else_alive) = self.condition_arms(cond, point);
        let mut then_point = point.clone();
        then_point.alive = then_alive;
        if let Some(then) = then {
            self.walk_stmts(then, &mut then_point);
        }
        let mut else_point = point.clone();
        else_point.alive = else_alive;
        if let Some(else_) = else_ {
            self.walk_stmts(else_, &mut else_point);
        }
        merge_live_arms(
            point,
            then_point,
            then.is_some_and(|arm| self.checker.block_diverges(arm)),
            else_point,
            else_.is_some_and(|arm| self.checker.block_diverges(arm)),
        );
    }

    fn condition_arms(
        &mut self,
        cond: &Expr,
        point: &mut SupplyPoint,
    ) -> (Option<Supply>, Option<Supply>) {
        match self.atom(cond, true, point) {
            AtomOutcome::NoInfo => {
                self.walk_expr(cond, point);
                (point.alive, point.alive)
            }
            AtomOutcome::Test { then, else_ } => {
                self.walk_cond_silently(cond, point);
                (self.arm_state(point, then), self.arm_state(point, else_))
            }
            AtomOutcome::Mention => {
                self.walk_cond_silently(cond, point);
                let silent = self.arm_state(point, Supply::Supplied);
                (silent, silent)
            }
        }
    }

    fn walk_expr(&mut self, expression: &Expr, point: &mut SupplyPoint) {
        ry_core::stack::grow(|| self.walk_expr_inner(expression, point))
    }

    fn walk_expr_inner(&mut self, expression: &Expr, point: &mut SupplyPoint) {
        if self.hit.is_some() {
            return;
        }
        match expression {
            Expr::Ident { name, span } => {
                if name == self.formal
                    && point.alive.is_some_and(|supply| supply.may_read_default())
                {
                    self.hit = Some(*span);
                }
            }
            Expr::Call { func, args, .. } => {
                // `missing(p)` inspects the promise without forcing it,
                // so its argument is not a read.
                if self.is_missing_test_call(func, args) {
                    return;
                }
                self.walk_expr(func, point);
                let captured =
                    crate::collect::captured_arguments(func, args, &self.checker.user_stubs);
                for (index, argument) in args.iter().enumerate() {
                    if captured.get(index).copied().unwrap_or(false) {
                        // A promise-capture helper (`substitute(p)`, ...)
                        // receives the promise unevaluated.
                        continue;
                    }
                    self.walk_expr(&argument.value, point);
                }
            }
            Expr::BinOp { op, lhs, rhs, .. } => match op {
                BinOpKind::Assign => self.walk_assign(lhs, rhs, point),
                // `p <<- v` assigns in an enclosing frame: the local
                // promise survives, so only the value side is walked.
                BinOpKind::SuperAssign => {
                    self.walk_expr(rhs, point);
                    if !matches!(lhs.as_ref(), Expr::Ident { .. }) {
                        self.walk_expr(lhs, point);
                    }
                }
                _ => {
                    self.walk_expr(lhs, point);
                    self.walk_expr(rhs, point);
                }
            },
            Expr::UnaryOp { expr, .. } => self.walk_expr(expr, point),
            Expr::Index { base, args, .. } => {
                self.walk_expr(base, point);
                for argument in args {
                    self.walk_expr(&argument.value, point);
                }
            }
            Expr::Function { .. } => {}
            Expr::Block { body, .. } => self.walk_stmts(body, point),
            Expr::If {
                cond, then, else_, ..
            } => self.walk_expr_if(cond, then, else_.as_deref(), point),
            Expr::Missing(_)
            | Expr::Unknown(_)
            | Expr::Logical(..)
            | Expr::Integer(..)
            | Expr::Double(..)
            | Expr::String(..)
            | Expr::Null(_)
            | Expr::Na(..) => {}
        }
    }

    /// With no `else`, the then-arm merges with the fall-through, so
    /// `lim <- if (missing(p)) e` keeps a proven-defaulted promise alive.
    fn walk_expr_if(
        &mut self,
        cond: &Expr,
        then: &Expr,
        else_: Option<&Expr>,
        point: &mut SupplyPoint,
    ) {
        let (then_alive, else_alive) = self.condition_arms(cond, point);
        let mut then_point = point.clone();
        then_point.alive = then_alive;
        self.walk_expr(then, &mut then_point);
        let mut else_point = point.clone();
        else_point.alive = else_alive;
        if let Some(else_) = else_ {
            self.walk_expr(else_, &mut else_point);
        }
        merge_live_arms(
            point,
            then_point,
            self.checker.expr_diverges_full(then),
            else_point,
            else_.is_some_and(|arm| self.checker.expr_diverges_full(arm)),
        );
    }

    /// Walk a condition that contains a supply test without reporting reads.
    fn walk_cond_silently(&mut self, cond: &Expr, point: &mut SupplyPoint) {
        let mut silent = point.clone();
        silent.alive = Some(Supply::Supplied);
        self.walk_expr(cond, &mut silent);
    }

    /// The arm-entry state for a decoded fact. A dead promise stays dead,
    /// because `missing()` after a rebinding is always FALSE.
    fn arm_state(&self, point: &SupplyPoint, supply: Supply) -> Option<Supply> {
        point.alive.is_some().then_some(supply)
    }

    /// Decompose `expr` under `polarity`, which says whether the enclosing
    /// condition asserts the subtree's truth.
    fn atom(&self, expr: &Expr, polarity: bool, point: &SupplyPoint) -> AtomOutcome {
        match expr {
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                ..
            } => self.atom(expr, !polarity, point),
            Expr::Call { func, args, .. } => {
                if self.is_missing_test_call(func, args) {
                    self.missing_atom(polarity)
                } else if self.mentions_supply_test(expr, point) {
                    AtomOutcome::Mention
                } else {
                    AtomOutcome::NoInfo
                }
            }
            Expr::Ident { name, .. } => match point.proxies.get(name).copied() {
                // The polarity and the cached negation XOR to the
                // then-arm's fact.
                Some(negated) => self.missing_atom(polarity != negated),
                None => AtomOutcome::NoInfo,
            },
            Expr::BinOp {
                op: BinOpKind::AndAnd,
                lhs,
                rhs,
                ..
            } => {
                // Decompose the operands positively, then swap the arms
                // when the composition is negated.
                let combined = self
                    .atom(lhs, true, point)
                    .combine_and(self.atom(rhs, true, point));
                if polarity {
                    combined
                } else {
                    combined.swapped()
                }
            }
            Expr::BinOp {
                op: BinOpKind::OrOr,
                lhs,
                rhs,
                ..
            } => {
                let combined = self
                    .atom(lhs, true, point)
                    .combine_or(self.atom(rhs, true, point));
                if polarity {
                    combined
                } else {
                    combined.swapped()
                }
            }
            other if self.mentions_supply_test(other, point) => AtomOutcome::Mention,
            _ => AtomOutcome::NoInfo,
        }
    }

    /// The test outcome for `missing(p)` under `polarity`.
    fn missing_atom(&self, polarity: bool) -> AtomOutcome {
        AtomOutcome::Test {
            then: if polarity {
                Supply::Defaulted
            } else {
                Supply::Supplied
            },
            else_: if polarity {
                Supply::Supplied
            } else {
                Supply::Defaulted
            },
        }
    }

    /// Whether `expr` is the direct call `missing(<this formal>)`, bare
    /// or namespace-qualified (`base::missing`).
    fn is_missing_test_call(&self, func: &Expr, args: &[Arg]) -> bool {
        args.len() == 1 && self.is_missing_head(func) && self.references_formal(&args[0].value)
    }

    fn is_missing_test(&self, expr: &Expr) -> bool {
        let Expr::Call { func, args, .. } = expr else {
            return false;
        };
        self.is_missing_test_call(func, args)
    }

    fn is_missing_head(&self, func: &Expr) -> bool {
        ident_name(func)
            .map(crate::semantic_lists::bare_name)
            .is_some_and(|name| name == "missing")
    }

    /// Whether any subexpression is a supply test for this formal: a
    /// `missing(p)` call or a reference to a cached proxy name.
    fn mentions_supply_test(&self, expr: &Expr, point: &SupplyPoint) -> bool {
        let mut found = false;
        let _ = walk_expr(
            expr,
            Walk::ALL,
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                match node {
                    AstNode::Expr(Expr::Call { func, args, .. })
                        if self.is_missing_test_call(func, args) =>
                    {
                        found = true;
                    }
                    AstNode::Expr(Expr::Ident { name, .. }) if point.proxies.contains_key(name) => {
                        found = true;
                    }
                    _ => {}
                }
                ControlFlow::Continue(Descend::Into)
            },
        );
        found
    }

    fn references_formal(&self, expr: &Expr) -> bool {
        let mut found = false;
        let _ = walk_expr(
            expr,
            Walk::ALL,
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                if let AstNode::Expr(Expr::Ident { name, .. }) = node
                    && name == self.formal
                {
                    found = true;
                }
                ControlFlow::Continue(Descend::Into)
            },
        );
        found
    }
}

/// Whether a `for` iterator is a literal `a:b` sequence or a single
/// non-`NA` literal, so the body runs at least once.
fn iterator_provably_nonempty(iter: &Expr) -> bool {
    match iter {
        Expr::Integer(..) | Expr::Double(..) | Expr::String(..) => true,
        Expr::BinOp {
            op: BinOpKind::Colon,
            lhs,
            rhs,
            ..
        } => {
            matches!(lhs.as_ref(), Expr::Integer(..) | Expr::Double(..))
                && matches!(rhs.as_ref(), Expr::Integer(..) | Expr::Double(..))
        }
        _ => false,
    }
}

/// Only arms that reach the continuation contribute their state.
fn merge_live_arms(
    point: &mut SupplyPoint,
    then: SupplyPoint,
    then_diverges: bool,
    else_: SupplyPoint,
    else_diverges: bool,
) {
    match (then_diverges, else_diverges) {
        (true, true) => point.alive = None,
        (true, false) => *point = else_,
        (false, true) => *point = then,
        (false, false) => *point = merge_points(&then, &else_),
    }
}

/// Merge two branch-end states. A path with no live promise contributes
/// none, and a cached test survives only when both paths bound the same
/// polarity.
fn merge_points(a: &SupplyPoint, b: &SupplyPoint) -> SupplyPoint {
    let mut proxies = HashMap::new();
    for (name, polarity) in &a.proxies {
        if b.proxies.get(name) == Some(polarity) {
            proxies.insert(name.clone(), *polarity);
        }
    }
    SupplyPoint {
        alive: match (a.alive, b.alive) {
            (Some(x), Some(y)) => Some(x.merge(y)),
            (None, other) | (other, None) => other,
        },
        proxies,
    }
}

impl Checker {
    /// RY108: an S3 `seq.<class>` method whose `to` formal has a non-NULL
    /// default uses `to` without a `missing(to)` guard. `seq.int` is not
    /// an S3 method of the generic.
    pub(crate) fn check_seq_defaulted_forward(
        &mut self,
        method_name: &str,
        params: &[Param],
        body: &[Stmt],
    ) {
        let method_name = semantic_argument_name(method_name);
        let Some(class) = method_name.strip_prefix("seq.") else {
            return;
        };
        if class.is_empty() || class == "int" {
            return;
        }
        // Without `...` the colliding specifiers cannot reach the method:
        // dispatch would reject them loudly instead.
        if !params.iter().any(|param| param.name == "...") {
            return;
        }
        let Some(to) = params.iter().find(|param| param.name == "to") else {
            return;
        };
        let Some(default) = &to.default else {
            return;
        };
        if matches!(default, Expr::Null(..)) {
            // A `NULL` default forwards loudly: `seq.default` errors
            // ("'to' must be of length 1"), so the wrongness is not
            // silent the way a value-carrying default is.
            return;
        }
        let Some(span) = first_unguarded_defaulted_use(self, body, "to") else {
            return;
        };
        self.emit(
            Severity::Warning,
            span,
            "RY108",
            format!(
                "`{method_name}()` uses `to` without a `missing(to)` check, but `to` is defaulted, so a call like `seq(x, length.out = n)` forwards the default as if supplied and hits `seq.default`'s argument precedence; guard the use with `missing(to)` first, as `seq.Date` does"
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str) -> Vec<Diagnostic> {
        let file = crate::tests::parse_file("supply.R", src);
        let mut checker = Checker::new("supply.R");
        checker.check(&file);
        checker.take_diagnostics()
    }

    fn fires(src: &str) -> bool {
        check(src).iter().any(|d| d.code == "RY108")
    }

    #[test]
    fn fires_on_the_hms_bug_shape() {
        // The unconditional cast forces the defaulted `to` before any
        // missing() test.
        assert!(fires(
            "as_hms <- function(x) x\n\
             seq.hms <- function(from = hms(1), to = hms(1), by = NULL, ...) {\n\
             \x20 to <- vec_cast(as_hms(to), numeric())\n\
             \x20 hms(seq(from, to, ...))\n\
             }\n"
        ));
    }

    #[test]
    fn fires_on_a_direct_forward_without_a_cast() {
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn fires_on_a_use_in_a_plain_condition() {
        // Forcing the defaulted `to` in a condition is the same
        // confusion, even without a seq forward.
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (to > from) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
    }

    #[test]
    fn fires_on_a_read_inside_the_proven_defaulted_branch() {
        // Entering the then-arm of `if (missing(to))` proves the value IS
        // the default: forwarding it there forwards the default (R:
        // seq(1, to=9-default, length.out=3) is 1 5 9).
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (missing(to)) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, ...)\n\
             \x20 }\n\
             }\n"
        ));
        // The negated cached test is the same fact through a proxy: the
        // else-arm of `if (m)` where `m <- !missing(to)` is defaulted.
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 m <- !missing(to)\n\
             \x20 if (m) {\n\
             \x20   seq(from, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
    }

    #[test]
    fn fires_when_a_while_condition_is_the_only_test() {
        // `missing(to) && n < 1` also fails on the `n >= 1` disjunct, so
        // leaving the loop does not prove `to` supplied (R: 1 5 9).
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 n <- 0\n\
             \x20 while (missing(to) && n < 1) {\n\
             \x20   n <- n + 1\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn negated_compositions_decode_by_de_morgan() {
        // `!(missing(to) || !ok)` holds only when `to` was supplied
        // (both disjuncts false), so the then-arm is proven supplied and
        // R skips it entirely when the value is defaulted (returns NULL).
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 ok <- TRUE\n\
             \x20 if (!(missing(to) || !ok)) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
        // The defensive spelling over a NULL check is the same guard.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (!(missing(to) || is.null(to))) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
        // `!(missing(to) && k)` leaves the arm reachable with the
        // defaulted value on the `!k` disjunct, so the use still
        // reports (R with k = FALSE enters the arm and forwards the
        // default: 1 5 9).
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 k <- FALSE\n\
             \x20 if (!(missing(to) && k)) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
        // And the else-arm of that guard is the `missing(to) && k`
        // path: a read there is a proven-defaulted read.
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 k <- TRUE\n\
             \x20 if (!(missing(to) && k)) {\n\
             \x20   seq(from, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
    }

    #[test]
    fn fires_after_a_statement_superassignment() {
        // `to <<- v` assigns in an enclosing frame; the local promise
        // survives (missing(to) stays TRUE) and the forward still hands
        // seq.default the default (R: 1 5 9).
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 to <<- 5\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
        // The right-to-left spelling lowers to the same wrapper.
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 5 ->> to\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn fires_for_a_nondiverging_expression_if_guard() {
        // `lim <- if (missing(to)) from + 1` does not rebind `to`: the
        // missing-path falls through to the continuation with the
        // defaulted promise still in place (R: 1 5 9).
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 lim <- if (missing(to)) from + 1\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_the_fixed_hms_shape() {
        // Every `to` use sits behind an `if (missing(to)) return(...)`
        // early exit, so the continuation is proven supplied.
        assert!(!fires(
            "as_hms <- function(x) x\n\
             seq.hms <- function(from = hms(1), to = hms(1), by = NULL, ...) {\n\
             \x20 from <- vec_cast(as_hms(from), numeric())\n\
             \x20 if (!is.null(by)) {\n\
             \x20   by <- vec_cast(as_hms(by), numeric())\n\
             \x20   if (missing(to)) {\n\
             \x20     return(hms(seq(from, by = by, ...)))\n\
             \x20   }\n\
             \x20   to <- vec_cast(as_hms(to), numeric())\n\
             \x20   return(hms(seq(from, to, by, ...)))\n\
             \x20 }\n\
             \x20 if (missing(to)) {\n\
             \x20   return(hms(seq(from, ...)))\n\
             \x20 }\n\
             \x20 to <- vec_cast(as_hms(to), numeric())\n\
             \x20 hms(seq(from, to, ...))\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_else_arm_guards() {
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (missing(to)) {\n\
             \x20   seq(from, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (!missing(to)) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, ...)\n\
             \x20 }\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_a_guarded_rebinding() {
        // `if (missing(to)) to <- fallback`: the then-path reads nothing
        // and rebinds (promise dead), the fall-through path is proven
        // supplied, so the later forward is safe on both paths.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (missing(to)) {\n\
             \x20   to <- from + 1\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_short_circuit_compositions() {
        // The condition's own `to` use is guarded by lazy `&&`, and the
        // then-arm requires `!missing(to)`.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (!missing(to) && to > from) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (missing(to) || to > 99) {\n\
             \x20   seq(from, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_a_cached_missing_test() {
        // seq.Date caches `mTo <- missing(to)`; the cached name guards.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 m_to <- missing(to)\n\
             \x20 if (m_to) {\n\
             \x20   seq(from, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 m_to <- !missing(to)\n\
             \x20 if (m_to) {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
    }

    #[test]
    fn an_undecodable_test_still_silences() {
        // The author checks `missing(to)` somehow; silence is the safe
        // reading even when the shape is not decodable.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (isTRUE(missing(to))) {\n\
             \x20   return(seq(from, ...))\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn a_later_guard_does_not_cover_an_earlier_use() {
        // Evaluation order: the early use happens before the guard.
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 x <- to\n\
             \x20 if (missing(to)) {\n\
             \x20   return(seq(from, ...))\n\
             \x20 }\n\
             \x20 seq(from, x, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_without_a_default_on_to() {
        // seq.Date / seq.POSIXt: `to` has no default, so nothing can be
        // confused with a supplied argument.
        assert!(!fires(
            "seq.widget <- function(from, to, by, length.out = NULL, ...) {\n\
             \x20 seq.int(from, to, length.out = length.out)\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_a_null_default_and_for_from() {
        assert!(!fires(
            "seq.widget <- function(from = 1, to = NULL, ...) {\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
        // `from` never competes with `...`-carried specifiers.
        assert!(!fires(
            "seq.widget <- function(from = 1, to, ...) {\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_seq_int_and_non_seq_names() {
        // `seq.int` is a separate function, not an S3 method of `seq`.
        assert!(!fires(
            "seq.int <- function(from = 1, to = from, ...) {\n\
             \x20 seq.int(from, to, ...)\n\
             }\n"
        ));
        assert!(!fires("summary.widget <- function(to = 9, ...) to\n"));
    }

    #[test]
    fn stays_silent_without_dots() {
        // Without `...`, colliding specifiers cannot enter the method.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9) {\n\
             \x20 seq(from, to)\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_when_to_is_only_defused_or_rebound() {
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 substitute(to)\n\
             }\n"
        ));
        // After `to <- expr` the name is a local; the default's value can
        // no longer be read through it.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 to <- 5\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn the_loop_variable_consumes_the_promise() {
        // R assigns the loop variable even on a zero-trip loop, so after
        // `for (to in ...)` the name can never read the default again
        // (R forwards 3 here, no bug).
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 for (to in 1:3) {\n\
             \x20   print(to)\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 for (to in NULL) {\n\
             \x20   print(to)\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn a_rebinding_on_one_branch_does_not_guard_the_fall_through() {
        // The fall-through path still reads the untested promise.
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (from > 0) {\n\
             \x20   to <- 5\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn closure_bodies_and_defaults_are_not_analyzed() {
        // A closure defined before the guard and called after it never
        // runs on the defaulted path (R: 1 2 3). Unguarded closures and
        // closure parameter defaults (`function(x = to)`) stay quiet too.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 helper <- function() seq(from, to, ...)\n\
             \x20 if (missing(to)) {\n\
             \x20   return(seq(from, ...))\n\
             \x20 }\n\
             \x20 helper()\n\
             }\n"
        ));
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 helper <- function() to\n\
             \x20 helper()\n\
             }\n"
        ));
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 helper <- function(x = to) x\n\
             \x20 helper()\n\
             }\n"
        ));
        // A same-named closure formal is that closure's own variable.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 helper <- function(to) to\n\
             \x20 helper(1)\n\
             }\n"
        ));
    }

    #[test]
    fn guard_via_stop_early_exit_is_recognized() {
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (missing(to)) {\n\
             \x20   stop('`to` is required')\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn stays_silent_for_a_diverging_expression_if_guard() {
        // `lim <- if (missing(to)) return(...)`: the then-arm returns to
        // the caller, so only the supplied fall-through reaches the
        // continuation (R: 1 2 3).
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 lim <- if (missing(to)) return(seq(from, ...))\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
        // A block arm containing return() diverges the same way.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 lim <- if (missing(to)) {\n\
             \x20   return(seq(from, ...))\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn provably_running_loop_bodies_keep_their_guards() {
        // repeat / while (TRUE) run their body at least once, and a
        // literal a:b iterator cannot be empty, so a guard-rebind inside
        // the body genuinely protects the post-loop use (R: no bug).
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 repeat {\n\
             \x20   if (missing(to)) {\n\
             \x20     to <- from + 1\n\
             \x20   }\n\
             \x20   break\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 for (i in 1:3) {\n\
             \x20   if (missing(to)) {\n\
             \x20     to <- from + 1\n\
             \x20   }\n\
             \x20 }\n\
             \x20 seq(from, to, ...)\n\
             }\n"
        ));
    }

    #[test]
    fn a_qualified_missing_test_is_recognized() {
        // `base::missing(to)` lowers with the qualified name, which the
        // bare-name split already strips.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 if (base::missing(to)) {\n\
             \x20   seq(from, ...)\n\
             \x20 } else {\n\
             \x20   seq(from, to, ...)\n\
             \x20 }\n\
             }\n"
        ));
    }

    #[test]
    fn fires_once_at_the_first_unguarded_use() {
        let diagnostics = check(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 a <- to + 1\n\
             \x20 b <- to + 2\n\
             \x20 seq(from, to, ...)\n\
             }\n",
        );
        let hits: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.code == "RY108").collect();
        assert_eq!(hits.len(), 1);
    }
}
