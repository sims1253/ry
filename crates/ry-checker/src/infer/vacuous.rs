//! Vacuous `all()` in validation guards (RY110) and the
//! emptiness-conditional aggregate values it rests on.
//!
//! `all(logical(0))` is `TRUE` (vacuous quantification), so a guard of
//! the shape `is.numeric(x) || all(is.na(x))` accepts any zero-length
//! non-numeric input: the predicate operand is FALSE for, say,
//! `character()`, but the `all()` operand is vacuously TRUE. The guard
//! then hands `x` to code that assumes the predicate's mode, which
//! fails downstream with an error about the callee rather than the
//! input. Runtime-verified from the hms audit (tidyverse/hms#231, fixed
//! in 046414d): `R/args.R`'s `is_numeric_or_na` let
//! `hms(seconds = character())` past validation, and the failure landed
//! inside `vec_cast()`/`data_frame()`.
//!
//! Two layers, matching the issue's suggested direction:
//!
//! * the literal-values half: over a definitely-empty argument the base
//!   aggregates evaluate to constants -- `all(empty)` is `TRUE` and
//!   `any(empty)` is `FALSE`, on every input mode (R: `all(character())`
//!   is `TRUE`). [`vacuous_aggregate_literal`] pins those constants from
//!   the same literal-construction proofs RY106 uses for its empty
//!   tests.
//! * the guard half: an `||` chain that combines a mode predicate over
//!   `x` with a bare `all(is.na(x))` operand arms a
//!   [`VacuousGuard`] for the accepted path. The diagnostic fires only
//!   when the vacuous-accept possibility -- an empty value failing the
//!   predicate, e.g. `character()` under `is.numeric` -- then reaches a
//!   downstream mode demand a typeshed stub declares (a parameter
//!   `type`, the same declarations RY092 checks). The demand gate is
//!   load-bearing for precision: `all()` over empty is often
//!   intentionally fine, and without a concrete downstream consumer
//!   the rule would cover every permissive guard in package code.
//!
//! The founding hms shape is interprocedural (the guard lives in the
//! `is_numeric_or_na` helper, the demand in `hms()`'s `vec_cast`), which
//! stays out of scope: the rule arms only guards whose accepted path is
//! visible in the same function -- an `if` branch, the `else` of a
//! negated guard, or the continuation of a rejecting `if`/`stopifnot`
//! whose rejection block diverges (`stop()`, `return()`).

use super::*;

/// The definite result of a base `all()`/`any()` call whose argument is
/// empty by construction. Vacuous quantification: every element of the
/// empty set satisfies `all`, and none satisfies `any`, regardless of
/// the elements' mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AggregateLiteral {
    True,
    False,
}

/// The definite value of an `all()`/`any()` call over an argument that
/// is provably empty: `all(logical(0))` is `TRUE`, `any(logical(0))` is
/// `FALSE`. Two proofs qualify:
///
/// * the argument is empty by construction -- a literal empty
///   (`logical(0)`, `character()`, `NULL`), the same shape proofs RY106
///   uses. An inferred `Length::Zero` on any other expression does not
///   qualify: inferred zeros are join artifacts as often as real
///   emptiness.
/// * the argument is `is.na(x)` and `x`'s recorded length is a
///   non-open-world `Zero` (a local proven empty, not a parameter or
///   narrowed binding): `is.na` mirrors `x`'s length, so the aggregate
///   sees a definitely empty vector. This is the proof RY110's guard
///   premise consumes.
///
/// A merely maybe-empty argument yields no literal: its `all()` is TRUE
/// only on the empty path. The `any()` half (`False`) has no production
/// consumer yet -- RY110's guard shape deliberately ignores `any()`
/// operands because `any(empty)` is FALSE and therefore rejects rather
/// than accepts -- so it stands as the evaluated twin of the `all()`
/// half: forward scaffolding for a future rule over vacuous `any()`
/// (`!any(...)`-style guard families), with its R semantics pinned by
/// the oracle claim fixture.
pub(crate) fn vacuous_aggregate_literal(
    checker: &Checker,
    callee: &Expr,
    args: &[Arg],
    scope: &Scope,
) -> Option<AggregateLiteral> {
    let name = ident_name(callee)?;
    let bare = crate::semantic_lists::bare_name(name);
    if !matches!(bare, "all" | "any") {
        return None;
    }
    // A shadowed aggregate may return anything.
    if !checker.resolves_to_base_lenient(name, scope) {
        return None;
    }
    // Only the plain single-argument form; extras keep the same vacuous
    // value, but the guard half reads exactly this shape, so the
    // literal half stays aligned with it.
    let [argument] = args else {
        return None;
    };
    if argument.name.is_some() {
        return None;
    }
    let definitely_empty = checker.test_definitely_empty(&argument.value, scope)
        || sole_is_na_ident(&argument.value).is_some_and(|var| {
            !checker.test_binding_is_open_world(var, scope)
                && scope
                    .get(ident_name(var).unwrap_or_default())
                    .is_some_and(|ty| ty.length == Length::Zero)
        });
    definitely_empty.then_some(match bare {
        "all" => AggregateLiteral::True,
        _ => AggregateLiteral::False,
    })
}

/// The identifier inside `is.na(<ident>)`, when the call has exactly one
/// unnamed argument.
fn sole_is_na_ident(expr: &Expr) -> Option<&Expr> {
    let Expr::Call { func, args, .. } = expr else {
        return None;
    };
    if ident_name(func)? != "is.na" {
        return None;
    }
    let [argument] = args.as_slice() else {
        return None;
    };
    (argument.name.is_none() && matches!(argument.value, Expr::Ident { .. }))
        .then_some(&argument.value)
}

/// One armed guard awaiting its downstream demand. Installed on the
/// accepted path; the first stub-declared parameter type inside
/// `accept` that rejects one of `vacuous_modes` while bound to `var`
/// fires RY110 at `all_span`.
pub(crate) struct VacuousGuard {
    var: String,
    all_span: Span,
    /// Byte range of the accepted path. The demand's argument span must
    /// lie inside it.
    accept: Span,
    /// The empty modes the guard vacuously admits: every atomic mode
    /// that fails the guard's predicates and is plausible for the
    /// guarded binding's recorded type. NULL is excluded on purpose:
    /// a dispatch-capable demand may accept a zero-length NULL (the
    /// RY092 humility for NULL actuals; arithmetic even recycles it),
    /// so NULL is the `is.null` guard family's own territory.
    vacuous_modes: Vec<Mode>,
    /// The binding's recorded type at the guard, for the generic-dispatch
    /// humility RY092 applies at demand sites.
    recorded: RType,
    /// The written predicate (`is.numeric`), for the diagnostic message.
    predicate_name: String,
    /// The guard's `all()` operand is TRUE for certain (the guarded
    /// binding is proven empty), not merely on the empty path.
    definite: bool,
    fired: bool,
}

/// Everything the guard parser learns about one `P(x) || all(is.na(x))`
/// chain.
struct VacuousAllSite<'a> {
    /// The guarded identifier.
    var: &'a Expr,
    all_span: Span,
    /// The `all()` operand's callee and arguments, so the armer can pin
    /// its literal value through [`vacuous_aggregate_literal`].
    all_callee: &'a Expr,
    all_args: &'a [Arg],
    /// Modes the chain's predicates over `var` cover; an empty value of
    /// any other plausible mode is accepted only through the vacuous
    /// `all()`.
    covered_modes: Vec<Mode>,
    /// The first covering predicate's bare name, for the message.
    predicate_name: String,
}

/// Parse an `||` chain into the vacuous-all guard shape: at least one
/// bare base `all(is.na(x))` operand, and at least one positive base
/// mode predicate over the SAME identifier -- the predicate is what
/// marks a validation-alternatives guard, and requiring it over the
/// guarded variable keeps the diagnostic's named predicate and
/// suggested rewrite about `x` itself (a predicate over another
/// variable only shrinks the vacuous set, but it would be reported as
/// the failed alternative). Extra operands (other predicates,
/// `is.null(x)`) are ignored; an `all()` operand guarded by an
/// emptiness check -- the fixed hms form
/// `length(x) > 0 && all(is.na(x))` -- is a `&&` expression and does not
/// match the bare-operand shape, so the corrected guard stays silent.
fn vacuous_all_site<'a>(
    checker: &Checker,
    chain: &'a Expr,
    scope: &Scope,
) -> Option<VacuousAllSite<'a>> {
    let mut leaves = Vec::new();
    collect_or_leaves(chain, &mut leaves);
    let mut all_leaf: Option<(&Expr, Span, &Expr, &[Arg])> = None;
    for leaf in &leaves {
        let Expr::Call { func, args, span } = leaf else {
            continue;
        };
        let Some(name) = ident_name(func) else {
            continue;
        };
        if crate::semantic_lists::bare_name(name) == "all" {
            // Only a bare `all(is.na(x))` carries the vacuous-accept
            // premise in the shape this rule pins; `any(is.na(x))` is
            // FALSE over empty and rejects instead of accepting.
            let var = bare_all_is_na_var(checker, name, args, scope)?;
            match all_leaf {
                Some((first, _, _, _)) if !same_ident(first, var) => return None,
                Some(_) => {}
                None => all_leaf = Some((var, *span, func, args)),
            }
        }
    }
    let (var, all_span, all_callee, all_args) = all_leaf?;
    let mut covered_modes = Vec::new();
    let mut predicate_name: Option<String> = None;
    for leaf in &leaves {
        let Expr::Call { func, args, .. } = leaf else {
            continue;
        };
        let Some(name) = ident_name(func) else {
            continue;
        };
        let bare = crate::semantic_lists::bare_name(name);
        // A positive single-identifier mode predicate over the guarded
        // variable: `is.numeric(x)`, `is.character(x)`, ... Class
        // predicates (`is.data.frame`, `inherits`) carry a class claim
        // the mode lattice cannot complement, so they do not count as
        // covering predicates; predicates over other variables are not
        // part of this guard at all.
        let Some(target) = narrow_predicate_target(bare) else {
            continue;
        };
        if target.class.has_known_class() {
            continue;
        }
        let [argument] = args.as_slice() else {
            continue;
        };
        let Expr::Ident { .. } = &argument.value else {
            continue;
        };
        if !same_ident(&argument.value, var) {
            continue;
        }
        if !checker.resolves_to_base_lenient(name, scope) {
            continue;
        }
        covered_modes.extend(modes_of_target(&target));
        predicate_name.get_or_insert_with(|| bare.to_string());
    }
    if covered_modes.is_empty() {
        return None;
    }
    Some(VacuousAllSite {
        var,
        all_span,
        all_callee,
        all_args,
        covered_modes,
        predicate_name: predicate_name.unwrap_or_else(|| "is.numeric".to_string()),
    })
}

/// Split an `||` chain into its leaf operands (either associativity).
fn collect_or_leaves<'e>(expr: &'e Expr, leaves: &mut Vec<&'e Expr>) {
    if let Expr::BinOp {
        op: BinOpKind::OrOr,
        lhs,
        rhs,
        ..
    } = expr
    {
        collect_or_leaves(lhs, leaves);
        collect_or_leaves(rhs, leaves);
    } else {
        leaves.push(expr);
    }
}

fn same_ident(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Ident { name: a, .. }, Expr::Ident { name: b, .. }) => a == b,
        _ => false,
    }
}

/// The guarded variable of a bare `all(is.na(x))` operand: base `all`
/// with exactly one unnamed argument, which is base `is.na` over exactly
/// one identifier. Anything else (`all(is.na(x), na.rm = TRUE)`,
/// `all(!is.na(x))`, a subscripted argument) does not match the pinned
/// shape.
fn bare_all_is_na_var<'a>(
    checker: &Checker,
    all_name: &str,
    args: &'a [Arg],
    scope: &Scope,
) -> Option<&'a Expr> {
    if !checker.resolves_to_base_lenient(all_name, scope) {
        return None;
    }
    let [argument] = args else {
        return None;
    };
    if argument.name.is_some() {
        return None;
    }
    let Expr::Call { func, args, .. } = &argument.value else {
        return None;
    };
    let name = ident_name(func)?;
    if crate::semantic_lists::bare_name(name) != "is.na"
        || !checker.resolves_to_base_lenient(name, scope)
    {
        return None;
    }
    let [inner] = args.as_slice() else {
        return None;
    };
    match &inner.value {
        Expr::Ident { .. } => Some(&inner.value),
        _ => None,
    }
}

/// `predicate_target` restricted to the storage modes whose complement
/// the lattice can enumerate. Returns an owned target; group predicates
/// (`is.numeric`) carry a union.
fn narrow_predicate_target(name: &str) -> Option<RType> {
    let target = narrow::predicate_target(name)?;
    is_storage_mode(target.mode).then_some(target)
}

/// The single mode and union members a predicate's target covers.
fn modes_of_target(target: &RType) -> Vec<Mode> {
    match target.mode {
        Mode::Union => target
            .members
            .as_ref()
            .map(|members| members.iter().map(|m| m.mode).collect())
            .unwrap_or_default(),
        mode => vec![mode],
    }
}

fn is_storage_mode(mode: Mode) -> bool {
    matches!(
        mode,
        Mode::Logical
            | Mode::Integer
            | Mode::Double
            | Mode::Complex
            | Mode::Character
            | Mode::Raw
            | Mode::List
            | Mode::Function
            | Mode::Union
    )
}

/// The empty modes the guard admits vacuously: atomic modes that fail
/// every covering predicate and are plausible for the binding's recorded
/// type. A recorded concrete mode or union restricts plausibility to its
/// own modes (a binding known `character` cannot arrive as `raw`);
/// opaque or unbound values keep every mode, the open-world default.
fn vacuous_accept_modes(recorded: Option<&RType>, covered: &[Mode]) -> Vec<Mode> {
    let pool = [
        Mode::Logical,
        Mode::Integer,
        Mode::Double,
        Mode::Complex,
        Mode::Character,
        Mode::Raw,
        Mode::List,
        Mode::Function,
    ];
    let plausible: Vec<Mode> = match recorded.map(|ty| ty.mode) {
        None | Some(Mode::Opaque) => pool.to_vec(),
        Some(Mode::Union) => recorded
            .and_then(|ty| ty.members.as_ref())
            .map(|members| members.iter().map(|m| m.mode).collect())
            .unwrap_or_default(),
        Some(mode) => vec![mode],
    };
    plausible
        .into_iter()
        .filter(|mode| !covered.contains(mode))
        .collect()
}

impl Checker {
    /// RY110's statement hook: recognize the vacuous-all guard in an
    /// `if` condition and arm it for the accepted path. The accepted
    /// path is the `then` branch for a positive condition; for a
    /// negated condition it is the `else` branch, or -- in the
    /// rejecting-guard idiom `if (!(G)) stop()` / `if (!(G)) return()` --
    /// the continuation after the `if`, but only when the rejection
    /// block provably diverges, which is what makes that continuation
    /// the guard-true path.
    pub(crate) fn check_vacuous_all_guard_stmt(
        &mut self,
        cond: &Expr,
        then: &[Stmt],
        else_: Option<&[Stmt]>,
        if_span: Span,
        scope: &Scope,
    ) {
        if self.discarding {
            return;
        }
        let (chain, negated) = match cond {
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                ..
            } => (expr.as_ref(), true),
            other => (other, false),
        };
        let Some(site) = vacuous_all_site(self, chain, scope) else {
            return;
        };
        let accept = if negated {
            match (else_, self.block_diverges(then)) {
                (Some(statements), _) => stmt_list_span(statements),
                (None, true) => self.stmt_continuations.get(&if_span).copied(),
                (None, false) => None,
            }
        } else {
            // A positive guard's `then` block is the accepted path. The
            // continuation after the `if` is NOT armed: it also runs
            // when the guard is FALSE, where the vacuous-accept premise
            // says nothing about `x`.
            stmt_list_span(then)
        };
        if let Some(accept) = accept {
            self.arm_vacuous_guard(&site, accept, scope);
        }
    }

    /// RY110's `stopifnot` hook: every `stopifnot(...)` argument is a
    /// guard whose accepted path is the continuation of the enclosing
    /// statement list. Rejection is `stopifnot`'s own semantics, so no
    /// divergence proof is needed.
    pub(crate) fn check_vacuous_all_stopifnot(&mut self, expr: &Expr, scope: &Scope) {
        if self.discarding {
            return;
        }
        let Expr::Call { func, args, span } = expr else {
            return;
        };
        let Some(name) = ident_name(func) else {
            return;
        };
        if crate::semantic_lists::bare_name(name) != "stopifnot"
            || !self.resolves_to_base_lenient(name, scope)
        {
            return;
        }
        let Some(&accept) = self.stmt_continuations.get(span) else {
            return;
        };
        for argument in args {
            if let Some(site) = vacuous_all_site(self, &argument.value, scope) {
                self.arm_vacuous_guard(&site, accept, scope);
            }
        }
    }

    /// Install one armed guard. The emptiness premise (`x` may be empty
    /// at runtime, open-world widening included) runs here against the
    /// live scope; a proven-empty binding additionally marks the accept
    /// definite through [`vacuous_aggregate_literal`]'s binding proof.
    fn arm_vacuous_guard(&mut self, site: &VacuousAllSite<'_>, accept: Span, scope: &Scope) {
        let recorded = scope.get(ident_name(site.var).unwrap_or_default()).cloned();
        let reference = recorded.clone().unwrap_or_else(RType::unknown);
        let definite = vacuous_aggregate_literal(self, site.all_callee, site.all_args, scope)
            == Some(AggregateLiteral::True);
        if !definite && !self.test_may_be_empty(site.var, &reference, scope) {
            return;
        }
        let vacuous_modes = vacuous_accept_modes(recorded.as_ref(), &site.covered_modes);
        if vacuous_modes.is_empty() {
            return;
        }
        // The fixpoint may walk a body more than once; one armed guard
        // per `all()` operand is all the diagnostic needs.
        if self
            .vacuous_guards
            .iter()
            .any(|guard| guard.all_span == site.all_span)
        {
            return;
        }
        let Some(var) = ident_name(site.var).map(str::to_owned) else {
            return;
        };
        self.vacuous_guards.push(VacuousGuard {
            var,
            all_span: site.all_span,
            accept,
            vacuous_modes,
            recorded: reference,
            predicate_name: site.predicate_name.clone(),
            definite,
            fired: false,
        });
    }

    /// Drop every armed guard over `name`: a reassignment between guard
    /// and demand replaces the guarded value, so the vacuous-accept
    /// possibility no longer describes what the demand receives. Runs in
    /// the pass-3 walk, which visits statements in source order, so
    /// demands textually before the rebinding keep their guards.
    pub(crate) fn note_vacuous_guard_rebind(&mut self, name: &str) {
        if !self.vacuous_guards.is_empty() {
            self.vacuous_guards.retain(|guard| guard.var != name);
        }
    }

    /// The `assign("x", v)` statement form of a rebind: the named local
    /// is replaced exactly as `x <- v` does, so its armed guards drop.
    pub(crate) fn note_vacuous_guard_assign_rebind(&mut self, expr: &Expr, scope: &Scope) {
        if self.vacuous_guards.is_empty() {
            return;
        }
        let Expr::Call { func, args, .. } = expr else {
            return;
        };
        let Some(name) = ident_name(func) else {
            return;
        };
        if crate::semantic_lists::bare_name(name) != "assign"
            || !self.resolves_to_base_lenient(name, scope)
        {
            return;
        }
        if let Some(Expr::String(name, _)) = args.first().map(|argument| &argument.value) {
            self.note_vacuous_guard_rebind(name);
        }
    }

    /// The demand half of RY110, called from the typeshed
    /// argument-validation stage for every parameter with a declared
    /// type. Fires when the argument is the guarded variable, the call
    /// site lies on the armed guard's accepted path, no nested function
    /// formal shadows the name between guard and demand (binding
    /// identity, not name equality: a closure parameter with the same
    /// spelling is a different binding), and the declared type rejects
    /// one of the guard's vacuous-accept modes.
    pub(crate) fn check_vacuous_guard_demand(
        &mut self,
        demand_name: &str,
        expected: &RType,
        argument: &Arg,
    ) {
        if self.discarding || self.vacuous_guards.is_empty() {
            return;
        }
        let Expr::Ident { name, span } = &argument.value else {
            return;
        };
        let contains =
            |outer: Span, inner: Span| outer.start <= inner.start && inner.end <= outer.end;
        // A formal of a nested function literal that contains the demand
        // but not the guard shadows the name for the demand site: the
        // `x` the demand sees is the closure's own parameter, never the
        // guarded binding (`vapply(x, function(x) sqrt(x), ...)` has no
        // failure path at all -- vapply over empty never calls the
        // lambda). A shadow that contains the guard as well means both
        // sites read the same formal, which is the same binding.
        let shadowed = |guard_span: Span| {
            self.formal_shadows.iter().any(|(formal, function_span)| {
                formal == name
                    && contains(*function_span, *span)
                    && !contains(*function_span, guard_span)
            })
        };
        let mut hit: Option<(Span, Mode, String, bool)> = None;
        for guard in &self.vacuous_guards {
            if guard.fired || guard.var != *name || shadowed(guard.all_span) {
                continue;
            }
            if !contains(guard.accept, *span) {
                continue;
            }
            // The same dispatch humility RY092 applies, decided by THIS
            // guard's recorded type: a generic demand may route a classed
            // or NULL value to a method that accepts it.
            if generic_argument_may_dispatch(&self.typeshed.globals, demand_name, &guard.recorded) {
                continue;
            }
            if let Some(mode) = guard
                .vacuous_modes
                .iter()
                .copied()
                .find(|mode| types_provably_incompatible(&RType::scalar(*mode), expected))
            {
                hit = Some((
                    guard.all_span,
                    mode,
                    guard.predicate_name.clone(),
                    guard.definite,
                ));
                break;
            }
        }
        let Some((all_span, mode, predicate_name, definite)) = hit else {
            return;
        };
        for guard in &mut self.vacuous_guards {
            if guard.all_span == all_span {
                guard.fired = true;
            }
        }
        if self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span == all_span && diagnostic.code == "RY110")
        {
            return;
        }
        let var = name.clone();
        let premise = if definite {
            format!("`{var}` is empty here, so `all(is.na({var}))` is TRUE")
        } else {
            format!("`all(is.na({var}))` is vacuously TRUE when `{var}` is empty")
        };
        // "cannot use as {label}" covers both demand behaviors the stubs
        // declare: the Math group errors on the empty non-numeric value,
        // while `mean()` warns ("argument is not numeric or logical")
        // and returns NA -- the value is unusable either way, but only
        // the first is a rejection.
        let label = expected_type_label(expected);
        self.emit(
            Severity::Warning,
            all_span,
            "RY110",
            format!(
                "{premise}, and the guard then accepts zero-length input that fails `{predicate_name}` (such as an empty {mode}) which `{demand_name}()` cannot use as {label}; guard the emptiness too: `{predicate_name}({var}) || (length({var}) > 0 && all(is.na({var})))`"
            ),
        );
    }
}

/// The root identifier of an assignment target: the bound name for a
/// plain `x <- v`, and the indexed root for complex targets
/// (`x[1] <- v` can coerce the whole vector's mode; `d$k <- v` can
/// change `d`'s shape). Counting the root for every index target is the
/// conservative direction for guard invalidation. Non-identifier roots
/// (calls) bind no name here.
pub(crate) fn assignment_root_name(target: &Expr) -> Option<&str> {
    match target {
        Expr::Index { base, .. } => assignment_root_name(base),
        Expr::Ident { name, .. } => Some(name),
        _ => None,
    }
}

/// Span covering a nonempty statement list; `None` for an empty one.
fn stmt_list_span(stmts: &[Stmt]) -> Option<Span> {
    let start = stmt_span(stmts.first()?);
    let end = stmt_span(stmts.last()?);
    Some(Span {
        start: start.start,
        end: end.end,
        line: start.line,
        col: start.col,
    })
}

pub(crate) fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Assign { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::FunctionDef { span, .. }
        | Stmt::Return { span, .. } => *span,
        Stmt::Expr(expression) => span_of(expression),
    }
}

/// Index the two AST facts RY110's demand correlation needs, in one
/// walk:
///
/// * for every statement, the byte range of the statements that follow
///   it in its enclosing list -- rejecting guards (`if (!(G)) stop(...)`,
///   `stopifnot(G)`) continue into that range on the accepted path;
/// * every function literal or definition's formal names with the
///   function's own span -- the binding-identity shadow set the demand
///   check consults (`function(x)` inside the accept range does not
///   consume an enclosing guard armed over its own `x`).
pub(crate) fn index_statement_continuations(
    stmts: &[Stmt],
    map: &mut std::collections::HashMap<Span, Span>,
    shadows: &mut Vec<(String, Span)>,
) {
    for (index, statement) in stmts.iter().enumerate() {
        if index + 1 < stmts.len() {
            let start = stmt_span(&stmts[index + 1]);
            let last = stmt_span(stmts.last().expect("index + 1 < len"));
            map.insert(
                stmt_span(statement),
                Span {
                    start: start.start,
                    end: last.end,
                    line: start.line,
                    col: start.col,
                },
            );
        }
        match statement {
            Stmt::Assign { value, .. } => index_expr_lists(value, map, shadows),
            Stmt::Expr(expression) => index_expr_lists(expression, map, shadows),
            Stmt::If { then, else_, .. } => {
                index_statement_continuations(then, map, shadows);
                if let Some(else_) = else_ {
                    index_statement_continuations(else_, map, shadows);
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                index_statement_continuations(body, map, shadows);
            }
            Stmt::FunctionDef { params, body, span } => {
                index_formals(params, *span, shadows);
                index_statement_continuations(body, map, shadows);
            }
            Stmt::Return { .. } => {}
        }
    }
}

/// Record each formal's name against the function's whole span. `...`
/// binds no name a call site could resolve to.
fn index_formals(params: &[Param], span: Span, shadows: &mut Vec<(String, Span)>) {
    for param in params {
        if param.name != "..." {
            shadows.push((param.name.clone(), span));
        }
    }
}

/// Statement lists nested inside expressions: function literals and
/// braced value blocks. Calls and operators carry their operands'
/// nested lists through the same recursion; function bodies get their
/// own continuation ranges, so a guard at the end of one function
/// cannot leak into a lexically following definition.
fn index_expr_lists(
    expr: &Expr,
    map: &mut std::collections::HashMap<Span, Span>,
    shadows: &mut Vec<(String, Span)>,
) {
    match expr {
        Expr::Function {
            params, body, span, ..
        } => {
            index_formals(params, *span, shadows);
            index_statement_continuations(body, map, shadows);
        }
        Expr::Block { body, .. } => index_statement_continuations(body, map, shadows),
        Expr::Call { func, args, .. } => {
            index_expr_lists(func, map, shadows);
            for argument in args {
                index_expr_lists(&argument.value, map, shadows);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            index_expr_lists(lhs, map, shadows);
            index_expr_lists(rhs, map, shadows);
        }
        Expr::UnaryOp { expr, .. } => index_expr_lists(expr, map, shadows),
        Expr::Index { base, args, .. } => {
            index_expr_lists(base, map, shadows);
            for argument in args {
                index_expr_lists(&argument.value, map, shadows);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            index_expr_lists(cond, map, shadows);
            index_expr_lists(then, map, shadows);
            if let Some(else_) = else_ {
                index_expr_lists(else_, map, shadows);
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Ident { .. }
        | Expr::Unknown(_)
        | Expr::Missing(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str) -> Vec<Diagnostic> {
        let file = crate::tests::parse_file("vacuous.R", src);
        let mut checker = Checker::new("vacuous.R");
        checker.check(&file);
        checker.take_diagnostics()
    }

    fn fires(src: &str) -> bool {
        check(src).iter().any(|d| d.code == "RY110")
    }

    /// One aggregate call from `src <- a <- <call>`; returns its callee
    /// expression, arguments, and the scope after checking the file.
    fn aggregate_call(src: &str) -> (Expr, Vec<Arg>, Scope) {
        let file = crate::tests::parse_file("vacuous.R", src);
        let mut checker = Checker::new("vacuous.R");
        let (_, scope) = checker.check_with_scope(&file);
        let Stmt::Assign { value, .. } = &file.stmts[0] else {
            panic!("fixture must assign the call");
        };
        let Expr::Call { func, args, .. } = value else {
            panic!("fixture must assign a call");
        };
        checker.take_diagnostics();
        ((**func).clone(), args.clone(), scope)
    }

    #[test]
    fn all_and_any_over_literal_empties_evaluate_to_constants() {
        // R 4.6: vacuous quantification is mode-independent. `all` over
        // any literal empty is TRUE, `any` is FALSE.
        let checker = Checker::new("vacuous.R");
        for (src, expected) in [
            ("a <- all(logical(0))\n", AggregateLiteral::True),
            ("a <- all(character())\n", AggregateLiteral::True),
            ("a <- all(NULL)\n", AggregateLiteral::True),
            ("a <- any(integer(length = 0))\n", AggregateLiteral::False),
            ("a <- any(logical(0))\n", AggregateLiteral::False),
        ] {
            let (func, args, scope) = aggregate_call(src);
            assert_eq!(
                vacuous_aggregate_literal(&checker, &func, &args, &scope),
                Some(expected),
                "src: {src}"
            );
        }
    }

    #[test]
    fn no_literal_value_for_maybe_empty_or_extra_arguments() {
        let checker = Checker::new("vacuous.R");
        // A parameter's emptiness is open-world knowledge, not a
        // definite construction; inferred zeros and nonempty values
        // likewise pin no constant.
        for src in [
            "a <- all(x)\nf <- function(x) NULL\n",
            "a <- all(logical(3))\n",
            "a <- all(is.na(x))\nf <- function(x) NULL\n",
            // Extra arguments keep the strict shape out.
            "a <- all(logical(0), na.rm = TRUE)\n",
        ] {
            let (func, args, scope) = aggregate_call(src);
            assert_eq!(
                vacuous_aggregate_literal(&checker, &func, &args, &scope),
                None,
                "src: {src}"
            );
        }
    }

    #[test]
    fn fires_on_the_hms_shape_with_a_downstream_demand() {
        // Local minimal form of tidyverse/hms#231 pre-fix: the guard
        // accepts `character()` vacuously and the demand rejects it.
        assert!(fires(
            "f <- function(seconds) {\n  if (is.numeric(seconds) || all(is.na(seconds))) {\n    sqrt(seconds)\n  }\n}\n"
        ));
        // Operand order does not matter.
        assert!(fires(
            "f <- function(x) {\n  if (all(is.na(x)) || is.numeric(x)) sqrt(x)\n}\n"
        ));
        // Extra operands in the chain keep the vacuous `all`.
        assert!(fires(
            "f <- function(x) {\n  if (is.null(x) || is.numeric(x) || all(is.na(x))) sqrt(x)\n}\n"
        ));
        // Every stub-declared numeric demand has the same shape.
        assert!(fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  mean(x)\n}\n"
        ));
        // A proven-empty local makes the accept definite: the message
        // says the guard HAS accepted, not that it may.
        assert!(fires(
            "v <- character()\nif (is.numeric(v) || all(is.na(v))) sqrt(v)\n"
        ));
    }

    #[test]
    fn fires_for_rejecting_if_and_stopifnot_continuations() {
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) stop(\"bad\")\n  sqrt(x)\n}\n"
        ));
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) {\n    stop(\"bad\")\n  }\n  round(x)\n}\n"
        ));
        assert!(fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  exp(x)\n}\n"
        ));
        // The else branch of a negated guard is the accepted path.
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) stop(\"bad\") else sqrt(x)\n}\n"
        ));
        // A demand nested inside the accepted branch fires too.
        assert!(fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  for (i in 1:3) {\n    out <- log(x)\n  }\n}\n"
        ));
    }

    #[test]
    fn stays_silent_without_a_downstream_mode_demand() {
        // The downstream-demand gate: `all()` over empty is often
        // intentionally permissive, and no stub-declared consumer means
        // no observable failure. `paste` declares no parameter types.
        assert!(!fires(
            "f <- function(x) {\n  if (is.numeric(x) || all(is.na(x))) {\n    total <- paste(x, collapse = \",\")\n  }\n}\n"
        ));
        // A demand that accepts the vacuous modes is no failure.
        assert!(!fires(
            "f <- function(x) {\n  if (is.numeric(x) || all(is.na(x))) {\n    print(x)\n  }\n}\n"
        ));
    }

    #[test]
    fn stays_silent_for_the_fixed_guard_and_adjacent_idioms() {
        // The fixed hms form: the `all()` is guarded by nonemptiness, so
        // the operand is not a bare `all(is.na(x))`.
        assert!(!fires(
            "f <- function(x) {\n  if (is.numeric(x) || (length(x) > 0 && all(is.na(x)))) sqrt(x)\n}\n"
        ));
        // `&&` guards do not accept vacuously: a FALSE predicate already
        // rejects the value.
        assert!(!fires(
            "f <- function(x) {\n  if (is.numeric(x) && all(is.na(x))) sqrt(x)\n}\n"
        ));
        // A bare `all(is.na(x))` guard is the skip-logic idiom; the
        // predicate operand is what marks a validation guard.
        assert!(!fires("f <- function(x) if (all(is.na(x))) sqrt(x)\n"));
        // Proven-nonempty values cannot hit the vacuous path.
        assert!(!fires(
            "v <- c(1, 2)\nif (is.numeric(v) || all(is.na(v))) sqrt(v)\n"
        ));
        // A binding whose recorded mode satisfies the predicate admits
        // nothing vacuously: its empty value is covered too.
        assert!(!fires(
            "f <- function(x) {\n  x <- as.numeric(x)\n  if (is.numeric(x) || all(is.na(x))) sqrt(x)\n}\n"
        ));
        // A recorded character binding is precisely the admitted shape:
        // `character(0)` fails `is.numeric` and passes `all(is.na())`
        // vacuously, so a rejecting demand is a true positive (below);
        // a demand that accepts character stays quiet.
        assert!(!fires(
            "f <- function(x) {\n  x <- paste0(x)\n  if (is.numeric(x) || all(is.na(x))) print(x)\n}\n"
        ));
        // Rebinding between guard and demand replaces the guarded value.
        assert!(!fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  x <- 1\n  sqrt(x)\n}\n"
        ));
        // A shadowed `all` is not the base aggregate.
        assert!(!fires(
            "all <- function(x) TRUE\nf <- function(x) {\n  if (is.numeric(x) || all(is.na(x))) sqrt(x)\n}\n"
        ));
        // A non-diverging rejection block leaves the accepted path
        // unproven.
        assert!(!fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) x <- NULL\n  sqrt(x)\n}\n"
        ));
    }

    #[test]
    fn a_demand_before_the_rebind_still_fires() {
        // The rebind invalidates the guard only for later demands; the
        // source-ordered walk checks the earlier demand first.
        assert!(fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  a <- sqrt(x)\n  x <- 1\n  b <- log(x)\n}\n"
        ));
    }

    #[test]
    fn fires_once_at_the_guard_not_per_demand() {
        // Two demands on the accepted path still report the single
        // guard site.
        let diagnostics = check(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  a <- sqrt(x)\n  b <- round(x)\n}\n",
        );
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == "RY110").count(),
            1,
            "diagnostics: {diagnostics:?}"
        );
    }

    #[test]
    fn demands_on_the_rejected_path_stay_silent() {
        // `sqrt(x)` inside the rejecting branch runs only when the guard
        // is FALSE; the accepted else demand is what fires.
        let diagnostics = check(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) {\n    sqrt(x)\n  } else {\n    round(x)\n  }\n}\n",
        );
        let sites: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "RY110")
            .map(|d| d.span.start)
            .collect();
        assert_eq!(sites.len(), 1, "diagnostics: {diagnostics:?}");
    }

    #[test]
    fn rejecting_return_guards_fire_in_every_syntactic_form() {
        // `return(...)` diverges exactly like `stop()`; unbraced, braced,
        // and bare `return()` all make the continuation the accepted
        // path. Runtime-true: a vacuously accepted `character()` makes
        // the guard TRUE, skips the return, and errors at `sqrt`.
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) return(NULL)\n  sqrt(x)\n}\n"
        ));
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) {\n    return(NULL)\n  }\n  sqrt(x)\n}\n"
        ));
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) return()\n  sqrt(x)\n}\n"
        ));
        // A return buried in a nested statement of the rejection block
        // still diverges the block.
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) {\n    if (verbose) return(NULL)\n    return(NA)\n  }\n  sqrt(x)\n}\nverbose <- TRUE\n"
        ));
        // `invisible()` returns a value and does not exit: the
        // continuation is not provably the accepted path.
        assert!(!fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) invisible(NULL)\n  sqrt(x)\n}\n"
        ));
    }

    #[test]
    fn qualified_base_return_diverges_like_the_bare_form() {
        // The shared view matches the callee's bare name, so an
        // explicit `base::return(...)` qualification rejects exactly
        // like the bare keyword (the same qualification `UseMethod`
        // already enjoys in `expr_diverges`).
        assert!(fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) base::return(NULL)\n  sqrt(x)\n}\n"
        ));
    }

    #[test]
    fn a_helpers_return_exits_the_helper_not_the_caller() {
        // `return` inside a collected helper returns a value to its
        // caller, and the caller's block continues past the call, so
        // the rejection arm below does not diverge and the continuation
        // stays unproven -- the helper-body recursion is return-blind
        // whatever view the outer query runs.
        assert!(!fires(
            "bail <- function(x) return(NULL)\nf <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) bail(x)\n  sqrt(x)\n}\n"
        ));
    }

    #[test]
    fn closure_parameters_do_not_consume_enclosing_guards() {
        // A nested function literal's identically named parameter is a
        // different binding: the demand inside operates on the closure's
        // own `x`, never the guarded value.
        assert!(!fires(
            "f <- function(x) {\n  if (!(is.numeric(x) || all(is.na(x)))) stop(\"bad\")\n  helper <- function(x) sqrt(x)\n  helper(2)\n}\n"
        ));
        // vapply over a vacuously accepted empty input returns
        // numeric(0) without ever invoking the lambda -- no failure
        // path exists at all.
        assert!(!fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  out <- vapply(x, function(x) sqrt(x), numeric(1))\n}\n"
        ));
        // A lambda WITHOUT a shadowing formal captures the guarded
        // binding; its demand is the same runtime defect. (A projected
        // use such as `sqrt(x[i])` stays silent per the bare-identifier
        // demand shape.)
        assert!(fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  later <- function() sqrt(x)\n  out <- later()\n}\n"
        ));
    }

    #[test]
    fn loop_rebinds_and_complex_rebinds_drop_guards() {
        // The loop variable rebinds `x` for the body and holds the final
        // iterated value afterwards; the guarded input never reaches the
        // demand.
        assert!(!fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  for (x in c(\"a\", \"b\")) total <- x\n  sqrt(x)\n}\n"
        ));
        // A subassign can coerce the whole vector's mode, so the root
        // name's guards drop like a plain rebind's.
        assert!(!fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  x[1] <- \"a\"\n  sqrt(x)\n}\n"
        ));
        // `assign("x", v)` rebinds the named local exactly as `x <- v`.
        assert!(!fires(
            "f <- function(x) {\n  stopifnot(is.numeric(x) || all(is.na(x)))\n  assign(\"x\", 1)\n  sqrt(x)\n}\n"
        ));
    }

    #[test]
    fn predicates_must_target_the_guarded_variable() {
        // A predicate over another variable neither covers nor names
        // this guard; there is no predicate over `x` here at all.
        assert!(!fires(
            "f <- function(x, y) {\n  if (is.character(y) || all(is.na(x))) sqrt(x)\n}\n"
        ));
        // With a genuine predicate over `x` in the chain, the foreign
        // predicate is ignored: the diagnostic names `is.numeric` and
        // the suggested rewrite touches `x`'s operand only.
        let diagnostics = check(
            "f <- function(x, y) {\n  if (is.character(y) || is.numeric(x) || all(is.na(x))) sqrt(x)\n}\n",
        );
        let messages: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "RY110")
            .map(|d| d.message.clone())
            .collect();
        assert_eq!(messages.len(), 1, "diagnostics: {diagnostics:?}");
        assert!(
            messages[0].contains("fails `is.numeric`"),
            "message must name the predicate over x: {}",
            messages[0]
        );
        assert!(
            !messages[0].contains("is.character"),
            "message must not name the predicate over y: {}",
            messages[0]
        );
    }
}
