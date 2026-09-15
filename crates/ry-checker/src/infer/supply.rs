//! Supplied-vs-defaulted state for function formals, and RY108
//! (seq-defaulted-forward).
//!
//! A defaulted formal has two runtime identities R keeps apart and ry's
//! types do not model: *supplied* (the caller passed an argument) and
//! *defaulted* (the caller omitted it and evaluation falls back to the
//! default expression). `missing(p)` is exactly the query that tells them
//! apart — it stays correct after the promise is forced, and collapses to
//! `FALSE` only once the name is reassigned (both verified against R
//! 4.6.1). Forcing the formal does not choose an identity: a defaulted `to`
//! evaluated inside the method is indistinguishable from a caller-supplied
//! one, which is the confusion the rules in this module report.
//!
//! The distinction is a capability several argument rules could reuse
//! (RY090/RY091/RY092/RY098 all reason about formals), so it lives here as
//! a small flow analysis over one formal rather than inside a rule:
//!
//! * A use of the formal is *unguarded* when some path from function entry
//!   reaches it with no `missing()` test seen — the path where the value
//!   may still be the default, read as if supplied.
//! * A `missing(p)` test at an `if` refines exactly like a type
//!   narrowing: entering either arm means the test ran, so both arms (and,
//!   when the tested arm diverges via `return`/`stop()`/`UseMethod`, the
//!   continuation) are guarded. `&&`/`||`/`!` compositions are decoded
//!   positionally; a test the walker cannot decode (inside a call, a
//!   comparison) still proves the author's awareness, which is the safe
//!   conclusion everywhere: arms and continuation count as checked.
//! * `m <- missing(p)` caches the test in a variable; `seq.Date`'s
//!   `mTo <- missing(to)` is the canonical example. Cached names are
//!   tracked with their polarity and decoded like the direct call until
//!   reassigned.
//! * `p <- value` rebinds the name: the value side is walked first (R
//!   evaluates it before the assignment), then the name stops denoting
//!   the promise — matching `missing(p)` collapsing to `FALSE` after a
//!   rebinding.
//!
//! Deliberately single-bit per path: the question a rule can act on is
//! only "can this read be a defaulted value the author has not tested?",
//! so branch merges OR the bit instead of merging a supply lattice. That
//! keeps `if (missing(p)) p <- fallback` from poisoning the fall-through
//! path, where the promise is still readable but proven supplied.
//!
//! RY108 applies the capability to the seq generic. `seq.hms`
//! (tidyverse/hms#231, pre-fix `R/hms.R:301`) cast and forwarded `to` —
//! which has a default — unconditionally, so `seq(hms(1), length.out = 3)`
//! re-entered `seq.default` with the *defaulted* `to` treated as a
//! supplied endpoint plus `...`-carried `length.out`: three identical
//! values, or "too many arguments" once `by` is forwarded too. `from` is
//! deliberately not analyzed: `seq.default` consumes `from` on every
//! path, so a defaulted `from` never competes with `...`-carried
//! specifiers — the upstream fix still casts `from` unconditionally. That
//! fix and `seq.Date`/`seq.POSIXt` guard every `to` use behind
//! `missing(to)`, which the flow analysis recognizes; a `to` without a
//! default (`seq.Date`) has nothing to confuse and stays silent.

use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr};
use std::ops::ControlFlow;

/// The per-point state of one formal: whether a read here can still be an
/// untested defaulted value, plus the cached `missing()` tests in scope.
/// Branch merges OR `unguarded` over the contributing paths and keep only
/// proxy bindings both paths agree on.
#[derive(Clone, Debug)]
struct SupplyPoint {
    unguarded: bool,
    /// Cached tests `m <- missing(p)` / `m <- !missing(p)`: name -> whether
    /// the binding holds the *negation* of the test.
    proxies: HashMap<String, bool>,
}

impl SupplyPoint {
    fn new() -> Self {
        Self {
            unguarded: true,
            proxies: HashMap::new(),
        }
    }
}

/// What an `if`/`while` condition proves about the formal's supply.
enum SupplyTest {
    /// The condition decodes to a `missing()` test (direct or cached):
    /// every arm — and the fall-through, once the tested arm diverges —
    /// has run the test.
    Decoded,
    /// The condition mentions a supply test in a shape the walker cannot
    /// decode (`isTRUE(missing(p))`, a comparison). The author
    /// demonstrated awareness; silence is the safe reading.
    Mention,
}

/// Outcome of decomposing one condition subtree.
enum AtomOutcome {
    Test,
    /// The subtree contains no supply test for this formal.
    NoInfo,
    Mention,
}

impl AtomOutcome {
    fn combine(self, rhs: Self) -> Self {
        match (self, rhs) {
            (AtomOutcome::Mention, _) | (_, AtomOutcome::Mention) => AtomOutcome::Mention,
            (AtomOutcome::Test, AtomOutcome::Test) => AtomOutcome::Test,
            (AtomOutcome::Test, AtomOutcome::NoInfo) | (AtomOutcome::NoInfo, AtomOutcome::Test) => {
                // `missing(p) && q`: the then-arm requires the test's
                // truth, but the else-arm is `!missing(p) || !q`, which
                // leaves the supply unknown on the `!q` disjunct. The
                // safest fact that survives both arms is the author's
                // awareness (a test exists on every path through the
                // condition), so this still counts as Mention-level
                // knowledge rather than a decoded test.
                AtomOutcome::Mention
            }
            (AtomOutcome::NoInfo, AtomOutcome::NoInfo) => AtomOutcome::NoInfo,
        }
    }
}

/// The first reference to `formal` in `body` that would evaluate while the
/// value may still be an untested default — the use that cannot tell a
/// supplied argument from a defaulted one. `None` means every use is
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
            Stmt::Assign { target, value, .. } => self.walk_assign(target, value, point),
            Stmt::Expr(expression) => self.walk_expr(expression, point),
            Stmt::If {
                cond, then, else_, ..
            } => self.walk_if(cond, Some(then), else_.as_deref(), point),
            Stmt::For {
                name, iter, body, ..
            } => {
                self.walk_expr(iter, point);
                // The loop variable rebinds the formal name on every
                // iteration, so reads inside the body see the iterator,
                // never the promise. The body may also not run: its
                // effects do not survive the loop.
                let mut inner = point.clone();
                if name == self.formal {
                    inner.unguarded = false;
                }
                self.walk_stmts(body, &mut inner);
            }
            Stmt::While { cond, body, .. } => {
                match self.classify_condition(cond, point) {
                    Some(_) => {
                        // Entering the body means the condition held, and
                        // leaving the loop means it failed or a `break`
                        // fired — every path ran the test.
                        self.walk_cond_silently(cond, point);
                        let mut inner = point.clone();
                        inner.unguarded = false;
                        self.walk_stmts(body, &mut inner);
                        point.unguarded = false;
                    }
                    None => {
                        self.walk_expr(cond, point);
                        let mut inner = point.clone();
                        self.walk_stmts(body, &mut inner);
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
                    point.unguarded = false;
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
                    point.unguarded = false;
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
                self.walk_arms(then, else_, point, point.unguarded);
            }
            Some(_) => {
                // A test's argument positions are guarded by evaluation
                // order: `missing(p) && p > 0` never evaluates `p > 0`
                // when the value is defaulted, and entering either arm
                // means the test ran.
                self.walk_cond_silently(cond, point);
                self.walk_arms(then, else_, point, false);
            }
        }
    }

    /// Walk both arms from forked states and merge the states that reach
    /// the continuation. `arm_unguarded` is the arm-entry bit for a
    /// decoded test (`false`: the test ran) or the inherited bit for a
    /// foreign condition. A diverging arm (`return`, `stop()`,
    /// `UseMethod`) contributes nothing; with no `else`, the implicit
    /// fall-through path inherits the pre-`if` state.
    fn walk_arms(
        &mut self,
        then: Option<&[Stmt]>,
        else_: Option<&[Stmt]>,
        point: &mut SupplyPoint,
        arm_unguarded: bool,
    ) {
        let mut then_point = point.clone();
        then_point.unguarded = arm_unguarded;
        if let Some(then) = then {
            self.walk_stmts(then, &mut then_point);
        }
        let then_diverges = then.is_some_and(|then| self.checker.block_diverges(then));
        match else_ {
            Some(else_statements) => {
                let mut else_point = point.clone();
                else_point.unguarded = arm_unguarded;
                self.walk_stmts(else_statements, &mut else_point);
                let else_diverges = self.checker.block_diverges(else_statements);
                *point = match (then_diverges, else_diverges) {
                    (true, true) => {
                        let mut dead = point.clone();
                        dead.unguarded = false;
                        dead
                    }
                    (true, false) => else_point,
                    (false, true) => then_point,
                    (false, false) => merge_points(&then_point, &else_point),
                };
            }
            None => {
                // With no `else`, the implicit fall-through path is the
                // condition's else-arm: it carries the arm-entry bit
                // (guarded for a decoded test, inherited otherwise).
                let mut fall_through = point.clone();
                fall_through.unguarded = arm_unguarded;
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
                if name == self.formal && point.unguarded {
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

    /// `if` in expression position. The arms are expressions, so the
    /// statement-level divergence rule (a `return` in a block arm) is not
    /// applied: the decoded-test case silences the arms and the merged
    /// continuation either way, which is the conservative direction for
    /// the rare `x <- if (missing(p)) return(...) else p` shape.
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
                self.walk_expr(then, point);
                if let Some(else_) = else_ {
                    self.walk_expr(else_, point);
                }
            }
            Some(_) => {
                self.walk_cond_silently(cond, point);
                let mut then_point = point.clone();
                then_point.unguarded = false;
                self.walk_expr(then, &mut then_point);
                match else_ {
                    Some(else_expr) => {
                        let mut else_point = point.clone();
                        else_point.unguarded = false;
                        self.walk_expr(else_expr, &mut else_point);
                        *point = merge_points(&then_point, &else_point);
                    }
                    None => {
                        *point = merge_points(&then_point, point);
                    }
                }
            }
        }
    }

    /// A nested closure's body: same-named formals shadow ours entirely;
    /// otherwise the body may force the captured promise when called, so
    /// reads count at the current state. Effects inside the closure body
    /// happen at call time and do not flow back to the definition site.
    fn walk_closure(&mut self, params: &[Param], body: &[Stmt], point: &mut SupplyPoint) {
        if params.iter().any(|param| param.name == self.formal) {
            return;
        }
        let mut inner = point.clone();
        for param in params {
            // A same-named closure formal is that closure's own variable;
            // whatever the outer scope cached under the name no longer
            // holds inside.
            inner.proxies.remove(&param.name);
        }
        self.walk_stmts(body, &mut inner);
    }

    /// Walk a condition that contains a supply test without reporting its
    /// argument reads.
    fn walk_cond_silently(&mut self, cond: &Expr, point: &mut SupplyPoint) {
        let mut silent = point.clone();
        silent.unguarded = false;
        self.walk_expr(cond, &mut silent);
    }

    // -- Condition classification -------------------------------------

    fn classify_condition(&self, cond: &Expr, point: &SupplyPoint) -> Option<SupplyTest> {
        match self.atom(cond, point) {
            AtomOutcome::Test => Some(SupplyTest::Decoded),
            AtomOutcome::Mention => Some(SupplyTest::Mention),
            AtomOutcome::NoInfo => None,
        }
    }

    /// Decompose `expr` into whether it *is* a supply test, contains none,
    /// or mentions one undecodably. `&&`/`||` combine conservatively: a
    /// test on one side of an `&&` proves the then-arm but not the
    /// else-arm, so the combination is only a Mention.
    fn atom(&self, expr: &Expr, point: &SupplyPoint) -> AtomOutcome {
        match expr {
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                ..
            } => self.atom(expr, point),
            Expr::Call { func, args, .. } => {
                if self.is_missing_test_call(func, args) {
                    AtomOutcome::Test
                } else if self.mentions_supply_test(expr, point) {
                    AtomOutcome::Mention
                } else {
                    AtomOutcome::NoInfo
                }
            }
            Expr::Ident { name, .. } => {
                if point.proxies.contains_key(name) {
                    AtomOutcome::Test
                } else {
                    AtomOutcome::NoInfo
                }
            }
            Expr::BinOp {
                op: BinOpKind::AndAnd | BinOpKind::OrOr,
                lhs,
                rhs,
                ..
            } => self.atom(lhs, point).combine(self.atom(rhs, point)),
            other if self.mentions_supply_test(other, point) => AtomOutcome::Mention,
            _ => AtomOutcome::NoInfo,
        }
    }

    /// Whether `expr` is the direct call `missing(<this formal>)`.
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

/// Merge two branch-end states into the state that survives both paths: a
/// defaulted read is possible if either path allows it, and a cached test
/// survives only when both paths bound the same polarity.
fn merge_points(a: &SupplyPoint, b: &SupplyPoint) -> SupplyPoint {
    let mut proxies = HashMap::new();
    for (name, polarity) in &a.proxies {
        if b.proxies.get(name) == Some(polarity) {
            proxies.insert(name.clone(), *polarity);
        }
    }
    SupplyPoint {
        unguarded: a.unguarded || b.unguarded,
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
    fn stays_silent_for_the_fixed_hms_shape() {
        // The 6fec3ad fix: every `to` use sits behind a
        // `if (missing(to)) return(...)` early exit, so the continuation
        // is proven guarded.
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
        // `if (missing(to)) to <- fallback`: the then-path reads the
        // default deliberately and the fall-through path is proven
        // supplied, so the later forward is guarded on both paths.
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
        // The condition's own `to` use is guarded by lazy `&&`.
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
    fn shadows_and_loops_keep_the_analysis_honest() {
        // A loop variable named `to` shadows the formal inside the body.
        assert!(!fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 for (to in 1:3) {\n\
             \x20   print(to)\n\
             \x20 }\n\
             \x20 seq(from, 5, ...)\n\
             }\n"
        ));
        // A rebinding on one branch does not guard the fall-through path,
        // where the untested promise is still readable.
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
    fn nested_closure_uses_count_until_shadowed() {
        assert!(fires(
            "seq.widget <- function(from = 1, to = 9, ...) {\n\
             \x20 helper <- function() to\n\
             \x20 helper()\n\
             }\n"
        ));
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
