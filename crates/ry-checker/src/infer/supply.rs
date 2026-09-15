//! Supplied-vs-defaulted state for function formals, and RY108
//! (seq-defaulted-forward).
//!
//! A defaulted formal has two runtime identities R keeps apart and ry's
//! types do not model: *supplied* (the caller passed an argument) and
//! *defaulted* (the caller omitted it and evaluation falls back to the
//! default expression). `missing(p)` is exactly the query that tells them
//! apart — it stays correct after the promise is forced, and collapses to
//! `FALSE` only once the name is reassigned or consumed as a loop
//! variable; a superassignment `p <<- v` leaves it intact (all verified
//! against R 4.6.1). Forcing the formal does not choose an identity: a
//! defaulted `to` evaluated inside the method is indistinguishable from a
//! caller-supplied one, which is the confusion the rules in this module
//! report.
//!
//! The distinction is a capability several argument rules could reuse
//! (RY090/RY091/RY092/RY098 all reason about formals), so it lives here as
//! a small flow analysis over one formal rather than inside a rule:
//!
//! * [`SupplyPoint::alive`] is the per-point state: `None` when every
//!   path reaching the point rebound the name (no live promise to read),
//!   otherwise the [`Supply`] fact for the paths where the promise is
//!   still readable. A read reports when the live promise is untested
//!   *or proven defaulted* — reading a proven-defaulted value forwards
//!   the default (the bug), so `if (missing(p)) seq(x, p, ...)` fires.
//!   Merging is path-correlated: a path that rebound the name contributes
//!   no live promise, which is what keeps
//!   `if (missing(p)) p <- fallback` from poisoning the supplied
//!   fall-through.
//! * A `missing(p)` test at an `if` refines exactly like a type
//!   narrowing: the then-arm gets the asserted fact, the else-arm (and,
//!   with no `else`, the fall-through) the negation, and a diverging arm
//!   (`return`, `stop()`, `UseMethod`) contributes nothing to the
//!   continuation. `&&`/`||`/`!` compositions decode positionally; cached
//!   tests (`m <- missing(p)`, `m <- !missing(p)` — `seq.Date`'s `mTo`)
//!   decode with their polarity until reassigned. A test the walker
//!   cannot decode (inside a call, a comparison) still proves the
//!   author's awareness, which silences safely.
//! * `p <- value` rebinds the name: the value side is walked first (R
//!   evaluates it before the assignment), then the promise is dead —
//!   matching `missing(p)` collapsing to `FALSE` after a rebinding.
//!   `p <<- value` (statement or expression) keeps the local promise.
//! * `for (p in ...)` consumes the promise unconditionally: R assigns
//!   the loop variable even on a zero-trip loop (`for (p in NULL)`
//!   leaves `missing(p)` `FALSE`), so the promise is dead after the
//!   loop.
//!
//! Documented concessions, in two classes. Recall-only (a guarded or
//! buggy read stays quiet): reads inside a nested closure's body or
//! parameter defaults are not analyzed at all — a closure defined before
//! a guard and called after it would otherwise fire on its
//! definition-site state although R never calls it on the defaulted
//! path; loop bodies whose iterator may be empty discard their exit
//! state (a guard inside such a body may not run), except
//! `repeat`/`while (TRUE)` and syntactically nonempty literal iterators,
//! where the body provably runs at least once and the first-iteration
//! exit approximates the post-loop state (a `break` before the body's
//! last statement included); `while` exit states ignore
//! `break`-mid-body paths; and a supply test's own condition is walked
//! silently, so `p > 0 || missing(p)` (the left operand forces the
//! promise unconditionally) stays quiet. Statically sound over-firing
//! (the defaulted value is reachable on some path, so a report is never
//! unproven, but runtimes that settle the unknown input can make the
//! guarded code clean): a loop body under an unknown `while` condition
//! may never run, so a guard inside it does not protect the post-loop
//! read; and the else-fact of a compound guard `A && missing(p)` (or its
//! De Morgan dual) cannot prove the `!missing(p)` disjunct when `A` is
//! unknown, so a read after such a guard reports even on runtimes where
//! `A`'s value made the guard total.
//!
//! RY108 applies the capability to the seq generic. `seq.hms`
//! (tidyverse/hms#231, pre-fix `R/hms.R:301`) cast and forwarded `to` —
//! which has a default — unconditionally, so `seq(hms(1), length.out = 3)`
//! re-entered `seq.default` with the *defaulted* `to` treated as a
//! supplied endpoint plus `...`-carried `length.out`: three identical
//! values, or "too many arguments" once `by` is forwarded too. `from` is
//! deliberately not analyzed: `seq.default` consumes `from` on every
//! path, so a defaulted `from` never competes with `...`-carried
//! specifiers — the upstream fix still casts `from` unconditionally.
//! `by` and `length.out` are excluded for the complementary reason: a
//! defaulted, forwarded `by` or `length.out` collides with a
//! `...`-carried competitor *loudly* ("too many arguments"), while the
//! `to` collision with `length.out` is silently wrong (`seq(1, 1,
//! length.out = 3)` is `1 1 1`), and ry reports silent wrongness, not
//! errors R already raises. That fix and `seq.Date`/`seq.POSIXt` guard
//! every `to` use behind `missing(to)`, which the flow analysis
//! recognizes; a `to` without a default (`seq.Date`) has nothing to
//! confuse and stays silent.

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
    /// The caller omitted the argument: the value IS the default. A read
    /// here forwards the default, which is exactly the bug.
    Defaulted,
}

impl Supply {
    /// The fact that survives both inputs being possible. Only equal
    /// facts merge to a fact; `Supplied` and `Defaulted` meet at
    /// `Untested`, which is also the absorbing element.
    fn merge(self, other: Self) -> Self {
        if self == other { self } else { Self::Untested }
    }

    /// The more specific of two facts: a non-`Untested` fact refines an
    /// `Untested` one. Used where both operands of `&&` must hold (both
    /// `||` operands must fail), so either proof settles the arm.
    fn refine(self, other: Self) -> Self {
        if self == Self::Untested { other } else { self }
    }

    /// Whether a read at this supply can forward a defaulted value.
    fn may_read_default(self) -> bool {
        !matches!(self, Self::Supplied)
    }
}

/// The per-point state of one formal: whether the promise is still
/// readable, and with what supply fact, plus the cached `missing()` tests
/// in scope. `alive == None` means every path reaching the point rebound
/// the name. Branch merges keep the path-correlated combination: rebound
/// paths contribute no live promise, and cached tests survive only when
/// both paths bound the same polarity.
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

/// What an `if`/`while` condition proves about the formal's supply.
enum SupplyTest {
    /// The condition decodes to a `missing()` test (direct or cached):
    /// (then-arm fact, else-arm fact).
    Decoded { then: Supply, else_: Supply },
    /// The condition mentions a supply test in a shape the walker cannot
    /// decode (`isTRUE(missing(p))`, a comparison). The author
    /// demonstrated awareness; silence is the safe reading.
    Mention,
}

/// Outcome of decomposing one condition subtree.
enum AtomOutcome {
    Test {
        then: Supply,
        else_: Supply,
    },
    /// The subtree contains no supply test for this formal.
    NoInfo,
    Mention,
}

impl AtomOutcome {
    /// The (then, else) supply facts, with `NoInfo` as the fully
    /// untested pair so the compositions are uniform lattice operations.
    fn facts(self) -> (Supply, Supply) {
        match self {
            AtomOutcome::Test { then, else_ } => (then, else_),
            AtomOutcome::NoInfo | AtomOutcome::Mention => (Supply::Untested, Supply::Untested),
        }
    }

    /// `a && b`: the then-arm requires both operands true (either proof
    /// settles it); the else-arm is `!a || !b`, where knowledge from only
    /// one disjunct does not survive the other being unknown.
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

    /// The facts with the arms exchanged: what the assertion-true path
    /// knew becomes the assertion-false path's knowledge. Applying the
    /// positive-polarity combinator and swapping under odd negation is
    /// De Morgan — `!(A || B)`'s true-path is `!A && !B` — which is how
    /// a negated composition decodes without duplicating the lattice
    /// operations for each polarity.
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

/// The first reference to `formal` in `body` that would evaluate while the
/// value may still be a defaulted one the author has not tested — the use
/// that cannot tell a supplied argument from a defaulted one, including a
/// read *inside* a proven-defaulted branch. `None` means every use is
/// guarded by a `missing()` test, reads a rebinding, or is defused
/// (`substitute(p)` and the promise-capture helpers).
///
/// A pure query: it records no diagnostics and does not touch scope, so
/// any rule can run it over a definition's body (RY108 does; the argument
/// rules are future consumers).
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
        match statement {
            Stmt::Assign { target, value, .. } => {
                // The statement form of `p <<- v` (and `v ->> p`) lowers
                // to Assign carrying a SuperAssign wrapper around the
                // value: R evaluates the value and assigns in an
                // enclosing frame, so the local promise survives
                // (`missing(p)` stays TRUE). The wrapper's left operand is
                // the target restated, never evaluated.
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
                    // The loop variable rebinds the formal name on every
                    // iteration, so reads inside the body see the
                    // iterator, never the promise.
                    inner.alive = None;
                }
                self.walk_stmts(body, &mut inner);
                if shadows {
                    // R assigns the loop variable even on a zero-trip
                    // loop (`for (p in NULL)` leaves `missing(p)` FALSE),
                    // so the promise is dead after the loop whatever the
                    // iterator.
                    point.alive = None;
                } else if iterator_provably_nonempty(iter) {
                    // The body runs at least once, so its first-iteration
                    // exit approximates the post-loop state (a `break`
                    // before the last statement included).
                    point.alive = inner.alive;
                    point.proxies = inner.proxies;
                }
                // Otherwise the iterator may be empty and the body may
                // never run: keep the entry state.
            }
            Stmt::While { cond, body, .. } => {
                // `repeat body` lowers to While with a constant-true
                // condition, which the main walker treats the same way
                // (`always_true`): the body provably runs at least once.
                let always_true = matches!(cond, Expr::Logical(true, _));
                match self.classify_condition(cond, point) {
                    Some(SupplyTest::Decoded { then, else_ }) => {
                        self.walk_cond_silently(cond, point);
                        let mut inner = point.clone();
                        inner.alive = self.arm_state(point, then);
                        self.walk_stmts(body, &mut inner);
                        // Leaving the loop requires the condition to fail;
                        // `break`-mid-body paths are approximated by this
                        // exit state.
                        point.alive = self.arm_state(point, else_);
                    }
                    Some(SupplyTest::Mention) => {
                        self.walk_cond_silently(cond, point);
                        let mut inner = point.clone();
                        inner.alive = self.arm_state(point, Supply::Supplied);
                        self.walk_stmts(body, &mut inner);
                        point.alive = self.arm_state(point, Supply::Supplied);
                    }
                    None => {
                        self.walk_expr(cond, point);
                        let mut inner = point.clone();
                        self.walk_stmts(body, &mut inner);
                        if always_true {
                            point.alive = inner.alive;
                            point.proxies = inner.proxies;
                        }
                    }
                }
            }
            Stmt::FunctionDef { params, body, .. } => self.walk_closure(params, body, point),
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    self.walk_expr(value, point);
                }
            }
        }
    }

    /// `target <- value`: R evaluates the value first, then rebinds. A
    /// plain name target is not evaluated (so `to <- v` is not a read of
    /// `to`); a complex target's substructure is, and complex assignment
    /// replaces the promise with the modified value.
    fn walk_assign(&mut self, target: &Expr, value: &Expr, point: &mut SupplyPoint) {
        self.walk_expr(value, point);
        match target {
            Expr::Ident { name, .. } => {
                if name == self.formal {
                    point.alive = None;
                } else {
                    // Record `m <- missing(p)` / `m <- !missing(p)` as a
                    // cached test; any other binding drops the cache.
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
        match self.classify_condition(cond, point) {
            None => {
                // The condition itself may force the formal (`if (to > 0)`),
                // which is exactly an unguarded use.
                self.walk_expr(cond, point);
                self.walk_arms(then, else_, point, point.alive, point.alive);
            }
            Some(SupplyTest::Decoded {
                then: then_fact,
                else_: else_fact,
            }) => {
                // A test's argument positions are guarded by evaluation
                // order: `missing(p) && p > 0` never evaluates `p > 0`
                // when the value is defaulted.
                self.walk_cond_silently(cond, point);
                let then_alive = self.arm_state(point, then_fact);
                let else_alive = self.arm_state(point, else_fact);
                self.walk_arms(then, else_, point, then_alive, else_alive);
            }
            Some(SupplyTest::Mention) => {
                self.walk_cond_silently(cond, point);
                let silent = self.arm_state(point, Supply::Supplied);
                self.walk_arms(then, else_, point, silent, silent);
            }
        }
    }

    /// Walk both arms from forked states and merge the states that reach
    /// the continuation. A diverging arm (`return`, `stop()`,
    /// `UseMethod`) contributes nothing; with no `else`, the implicit
    /// fall-through path carries the condition's else-fact (for a decoded
    /// test) or the inherited state (for a foreign condition).
    fn walk_arms(
        &mut self,
        then: Option<&[Stmt]>,
        else_: Option<&[Stmt]>,
        point: &mut SupplyPoint,
        then_alive: Option<Supply>,
        else_alive: Option<Supply>,
    ) {
        let mut then_point = point.clone();
        then_point.alive = then_alive;
        if let Some(then) = then {
            self.walk_stmts(then, &mut then_point);
        }
        let then_diverges = then.is_some_and(|then| self.stmts_diverge(then));
        match else_ {
            Some(else_statements) => {
                let mut else_point = point.clone();
                else_point.alive = else_alive;
                self.walk_stmts(else_statements, &mut else_point);
                let else_diverges = self.stmts_diverge(else_statements);
                *point = match (then_diverges, else_diverges) {
                    (true, true) => {
                        let mut dead = point.clone();
                        dead.alive = None;
                        dead
                    }
                    (true, false) => else_point,
                    (false, true) => then_point,
                    (false, false) => merge_points(&then_point, &else_point),
                };
            }
            None => {
                let mut fall_through = point.clone();
                fall_through.alive = else_alive;
                *point = if then_diverges {
                    fall_through
                } else {
                    merge_points(&then_point, &fall_through)
                };
            }
        }
    }

    fn walk_expr(&mut self, expression: &Expr, point: &mut SupplyPoint) {
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
                let captured = crate::collect::captured_arguments(func, args);
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
            Expr::Function { params, body, .. } => self.walk_closure(params, body, point),
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

    /// `if` in expression position. Unlike the statement form, a
    /// non-diverging then-arm does not rebind anything, so the
    /// continuation merges the then-arm's state (a proven-defaulted
    /// promise survives `lim <- if (missing(p)) e` with no `else`) with
    /// the fall-through's else-fact — which is why that shape followed by
    /// a `p` forward still reports. A diverging then-arm (`if
    /// (missing(p)) return(...)`) leaves only the fall-through reaching
    /// the continuation and is silent, mirroring the statement form.
    fn walk_expr_if(
        &mut self,
        cond: &Expr,
        then: &Expr,
        else_: Option<&Expr>,
        point: &mut SupplyPoint,
    ) {
        match self.classify_condition(cond, point) {
            None => {
                self.walk_expr(cond, point);
                let mut then_point = point.clone();
                self.walk_expr(then, &mut then_point);
                let then_diverges = self.expr_diverges_strict(then);
                self.merge_expr_arms(then_point, then_diverges, else_, point, point.alive);
            }
            Some(SupplyTest::Decoded {
                then: then_fact,
                else_: else_fact,
            }) => {
                self.walk_cond_silently(cond, point);
                let mut then_point = point.clone();
                then_point.alive = self.arm_state(point, then_fact);
                self.walk_expr(then, &mut then_point);
                let then_diverges = self.expr_diverges_strict(then);
                let else_alive = self.arm_state(point, else_fact);
                self.merge_expr_arms(then_point, then_diverges, else_, point, else_alive);
            }
            Some(SupplyTest::Mention) => {
                self.walk_cond_silently(cond, point);
                let silent = self.arm_state(point, Supply::Supplied);
                let mut then_point = point.clone();
                then_point.alive = silent;
                self.walk_expr(then, &mut then_point);
                let then_diverges = self.expr_diverges_strict(then);
                self.merge_expr_arms(then_point, then_diverges, else_, point, silent);
            }
        }
    }

    /// Merge the expression-if arms into `point`. `then_diverges` drops
    /// the then-arm's state; with no `else`, the fall-through carries
    /// `fall_alive` (the condition's else-fact, or the pre-`if` state for
    /// a foreign condition).
    fn merge_expr_arms(
        &mut self,
        then_point: SupplyPoint,
        then_diverges: bool,
        else_: Option<&Expr>,
        point: &mut SupplyPoint,
        fall_alive: Option<Supply>,
    ) {
        match else_ {
            Some(else_expr) => {
                let mut else_point = point.clone();
                else_point.alive = fall_alive;
                self.walk_expr(else_expr, &mut else_point);
                let else_diverges = self.expr_diverges_strict(else_expr);
                *point = match (then_diverges, else_diverges) {
                    (true, true) => {
                        let mut dead = point.clone();
                        dead.alive = None;
                        dead
                    }
                    (true, false) => else_point,
                    (false, true) => then_point,
                    (false, false) => merge_points(&then_point, &else_point),
                };
            }
            None => {
                let mut fall_through = point.clone();
                fall_through.alive = fall_alive;
                *point = if then_diverges {
                    fall_through
                } else {
                    merge_points(&then_point, &fall_through)
                };
            }
        }
    }

    /// A nested closure's body and parameter defaults are not analyzed.
    /// They evaluate when the closure is CALLED, not here, so the
    /// definition-site supply state would misclassify both directions: a
    /// closure defined before a guard and called after it would fire
    /// although R never calls it on the defaulted path, and a default
    /// like `function(x = to)` forces the promise at call time. Reads
    /// that only ever happen inside closures therefore stay silent; the
    /// enclosing method's own reads still report.
    fn walk_closure(&mut self, _params: &[Param], _body: &[Stmt], _point: &mut SupplyPoint) {}

    /// Walk a condition that contains a supply test without reporting its
    /// argument reads.
    fn walk_cond_silently(&mut self, cond: &Expr, point: &mut SupplyPoint) {
        let mut silent = point.clone();
        silent.alive = Some(Supply::Supplied);
        self.walk_expr(cond, &mut silent);
    }

    /// The arm-entry state for a decoded fact: a dead promise stays dead
    /// (a `missing()` test after a rebinding is constantly FALSE, so the
    /// tested arm is unreachable and the other arm inherits the local).
    fn arm_state(&self, point: &SupplyPoint, supply: Supply) -> Option<Supply> {
        point.alive.is_some().then_some(supply)
    }

    // -- Condition classification -------------------------------------

    fn classify_condition(&self, cond: &Expr, point: &SupplyPoint) -> Option<SupplyTest> {
        match self.atom(cond, true, point) {
            AtomOutcome::Test { then, else_ } => Some(SupplyTest::Decoded { then, else_ }),
            AtomOutcome::Mention => Some(SupplyTest::Mention),
            AtomOutcome::NoInfo => None,
        }
    }

    /// Decompose `expr` under `polarity` (whether the enclosing condition
    /// asserts the subtree's truth). `point` supplies the proxy bindings
    /// in scope at the condition.
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
                // The binding holds `missing(p)` when not negated and
                // `!missing(p)` when negated; the condition asserts the
                // binding's value, so the two flags XOR to the supply the
                // then-arm sees.
                Some(negated) => self.missing_atom(polarity != negated),
                None => AtomOutcome::NoInfo,
            },
            Expr::BinOp {
                op: BinOpKind::AndAnd,
                lhs,
                rhs,
                ..
            } => {
                // The operands appear positively inside the composition
                // (polarity only flips through `!`), so they decompose at
                // their own truth and the positive combinator applies; an
                // assertion that NEGATES the composition then takes the
                // De Morgan dual by exchanging the arms.
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

    /// The test outcome for a `missing(p)` fact, under `polarity`: the
    /// then-arm sees the asserted fact, the else-arm the negation.
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

    /// Whether every path through these statements stops the enclosing
    /// function. The main walker's divergence query misses `return(...)`
    /// here: the parser never produces `Stmt::Return` (it lowers the
    /// keyword to an ordinary call), and `return` has no `no_return`
    /// stub, so this strict variant adds the call form on top of
    /// [`Checker::expr_diverges`] (`stop()` via its stub, `UseMethod`,
    /// diverging collected helpers).
    fn stmts_diverge(&self, stmts: &[Stmt]) -> bool {
        stmts.iter().any(|statement| match statement {
            Stmt::Return { .. } => true,
            Stmt::Expr(expression) => self.expr_diverges_strict(expression),
            Stmt::If { then, else_, .. } => else_
                .as_ref()
                .is_some_and(|else_| self.stmts_diverge(then) && self.stmts_diverge(else_)),
            _ => false,
        })
    }

    /// Whether evaluating `expr` stops the enclosing function: blocks and
    /// expression `if`s recurse structurally, `return(...)` parses as an
    /// ordinary call in expression position (the shape that matters
    /// here: `lim <- if (missing(p)) return(...)`), and everything else
    /// defers to [`Checker::expr_diverges`].
    fn expr_diverges_strict(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Call { func, .. }
                if ident_name(func)
                    .map(crate::semantic_lists::bare_name)
                    .is_some_and(|name| name == "return") =>
            {
                true
            }
            Expr::Block { body, .. } => self.stmts_diverge(body),
            Expr::If { then, else_, .. } => else_.as_ref().is_some_and(|else_| {
                self.expr_diverges_strict(then) && self.expr_diverges_strict(else_)
            }),
            other => self
                .checker
                .expr_diverges(other, &mut std::collections::HashSet::new()),
        }
    }
}

/// Whether a `for` iterator is syntactically provably nonempty: a
/// literal-only `a:b` sequence, or a single non-`NA` literal (length 1).
/// Anything else may be empty, and the loop body's effects then may never
/// run.
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

/// Merge two branch-end states into the state that survives both paths:
/// a path with no live promise contributes none (its reads see a local),
/// live paths merge their supply facts, and a cached test survives only
/// when both paths bound the same polarity.
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
    /// default uses `to` without a `missing(to)` guard.
    ///
    /// The seq generic forwards `...`-carried specifiers into the method,
    /// so a call like `seq(x, length.out = n)` enters the method with the
    /// defaulted `to` plus `length.out`; forwarding or casting that `to`
    /// unconditionally hands `seq.default` a defaulted endpoint it cannot
    /// distinguish from a supplied one. Only `to` is analyzed (see the
    /// module docs); `seq.int` is not an S3 method of the generic.
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
        // tidyverse/hms#231 pre-fix R/hms.R:301: the unconditional cast
        // forces the defaulted `to` before any missing() test.
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
        // The 6fec3ad fix: every `to` use sits behind a
        // `if (missing(to)) return(...)` early exit, so the continuation
        // is proven supplied.
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
        // runs on the defaulted path (R: 1 2 3), so its body must not
        // fire; suppression also covers unguarded closures and closure
        // parameter defaults (`function(x = to)`), a documented recall
        // concession.
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
