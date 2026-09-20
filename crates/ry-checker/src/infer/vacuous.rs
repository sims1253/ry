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
//! `is_numeric_or_na` helper, the demand in `hms()`'s `vec_cast`), and
//! issue #479 covers its same-file helper half: a single-formal function
//! the checked file defines whose body is (or returns) the recognized
//! chain registers a [`VacuousHelper`], armed at call sites that pass the
//! guarded value onward -- a direct `H(v)` guard condition,
//! `stopifnot(H(v))`, or a `map`-family application whose result is
//! reduced with `all()` (the args.R `map_lgl` + `all(valid)` + rejection
//! form). The downstream demand must still sit in the applying function
//! under a stub-declared parameter type: a validator-summary hop (one
//! function validates, another demands) and a cross-file helper
//! application stay silent -- the latter because the diagnostic could
//! not point at the helper's own span without attributing a foreign byte
//! range to the consuming file. The `vec_cast` demand is relational
//! (which `x` modes are legal depends on `to`: `vec_cast(x, NULL)`
//! returns `x` unchanged), so the ry-side overlay stub
//! (`crates/ry-typeshed/overlay/vctrs.json`) pins only R's formals and
//! declares no parameter types -- the numeric-target `vec_cast` shape of
//! the founding hms defect is a recorded capability gap, not a contract
//! this rule can prove; the validator-summary hop stays out of scope for
//! the #351 flow-sensitivity cycle, and upstreaming the stub is the
//! maintainer's call.

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
    /// The guard condition's own span: the `if`/`stopifnot` condition for
    /// an inline guard, the helper-call (or `all()` reduction) condition
    /// for a helper-armed guard. The binding-identity shadow check reads
    /// this span -- a nested formal shadowing the name must contain the
    /// demand without containing the guard site *in the guard's own
    /// function*. For helper guards the diagnostic still points at
    /// `all_span` (the defect is the helper's definition), while this
    /// span anchors the shadow logic to the caller's guard site.
    site_span: Span,
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

/// One registered guard-helper (issue #479): a single-formal function
/// whose body is (or returns) the recognized vacuous-all chain over that
/// formal, e.g. hms's `is_numeric_or_na <- function(x) is.numeric(x) ||
/// all(is.na(x))`. The fact is keyed by the helper's name and consumed at
/// call sites that pass the guarded value onward -- a direct `H(v)` guard
/// condition, or a `map`-family application whose result is reduced with
/// `all()` -- where it arms an ordinary [`VacuousGuard`] over the actual
/// argument. The diagnostic still points at the helper's `all()` operand:
/// that definition is the defect (hms fixed the helper, not its callers),
/// and one site per helper is all the diagnostic needs.
#[derive(Debug, Clone)]
pub(crate) struct VacuousHelper {
    /// The helper's `all()` operand span: the diagnostic site.
    all_span: Span,
    /// Modes the chain's predicates cover; complements the caller's
    /// recorded type into the vacuous-accept set at each arm site.
    covered_modes: Vec<Mode>,
    /// The first covering predicate's bare name, for the message.
    predicate_name: String,
}

/// Elementwise-validation provenance (issue #479): `result` is bound by a
/// `map`-family call applying a guard-helper to `data`
/// (`valid <- map_lgl(args, is_numeric_or_na)`). An `all(result)` guard
/// that rejects then proves every element of `data` passed the helper,
/// so the `if`/`stopifnot` hooks arm the helper's guard over `data`.
#[derive(Debug, Clone)]
pub(crate) struct VacuousMapProvenance {
    /// The mapped collection (a bare identifier at the `map` site).
    data: String,
    /// The applied guard-helper's registry name.
    helper: String,
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
    /// RY110's helper registry (issue #479): index every single-formal
    /// function the file being checked defines at the top level whose
    /// body is (or returns) the recognized vacuous-all chain over that
    /// formal. Runs once in the pass-3 prologue from the file's own
    /// statements, so use-before-def order cannot hide a helper (hms
    /// defines `check_args` before `is_numeric_or_na`) while the scan
    /// stays O(the file's own functions) -- the `scaling_project_size`
    /// perf budget guards this. The registry is file-local by
    /// construction: a helper in another file never arms here, so a
    /// cross-file diagnostic can never carry a foreign byte span (the
    /// emitter stamps the consuming file's path). The parse runs against
    /// an empty scope with the checker's fully populated tables, which
    /// resolves top-level shadowing (`all <- function...` disqualifies)
    /// the same way the lenient base-resolution ladder does for inline
    /// guards.
    pub(crate) fn index_vacuous_helpers(&mut self, stmts: &[Stmt]) {
        use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
        use std::ops::ControlFlow;
        // Top-level binding statements, mirroring `collect_fns`: the
        // traversal enters `if`/`for`/`while` bodies but never expression
        // interiors, so a helper bound in a top-level branch registers
        // while nested closures never do (their bare names would resolve
        // against the wrong environment anyway).
        let mut defs: Vec<(String, Vec<Param>, Vec<Stmt>)> = Vec::new();
        let _ = walk_stmts(
            stmts,
            Walk {
                control_tests: false,
                ..Walk::ALL
            },
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                let AstNode::Stmt(statement) = node else {
                    return ControlFlow::Continue(Descend::Skip);
                };
                if let Stmt::Assign { target, value, .. } = statement
                    && let (Some(name), Expr::Function { params, body, .. }) =
                        (binding_name(target), value)
                {
                    defs.push((name.to_string(), params.clone(), body.clone()));
                }
                ControlFlow::Continue(match statement {
                    Stmt::If { .. } | Stmt::For { .. } | Stmt::While { .. } => Descend::Into,
                    _ => Descend::Skip,
                })
            },
        );
        let empty = Scope::default();
        for (name, params, body) in &defs {
            let [param] = params.as_slice() else {
                continue;
            };
            if param.name == "..." {
                continue;
            }
            let [statement] = body.as_slice() else {
                continue;
            };
            // The body is the chain (implicit return) or returns it.
            // `Stmt::Return`'s value is `Option` only because bare
            // `return()` exits with NULL; that form returns no chain.
            // The parser lowers the `return` keyword to an ordinary
            // call, so a braced `return(chain)` body arrives as
            // `Stmt::Expr(Call)` -- unwrap that spelling too.
            let chain = match statement {
                Stmt::Expr(chain) => Some(chain),
                Stmt::Return {
                    value: Some(chain), ..
                } => Some(chain),
                _ => None,
            };
            let chain = chain.and_then(|chain| match chain {
                Expr::Call { func, args, .. }
                    if ident_name(func).is_some_and(|name| {
                        name == "return" || crate::semantic_lists::bare_name(name) == "return"
                    }) =>
                {
                    match args.as_slice() {
                        [argument] if argument.name.is_none() => Some(&argument.value),
                        _ => None,
                    }
                }
                chain => Some(chain),
            });
            let Some(chain) = chain else {
                continue;
            };
            let Some(site) = vacuous_all_site(self, chain, &empty) else {
                continue;
            };
            // The chain must guard the helper's own formal: only then
            // does applying the helper validate the passed value.
            if ident_name(site.var) != Some(param.name.as_str()) {
                continue;
            }
            // A shadowed-all twin of the helper body (the corpus
            // `own_all` shape lifted interprocedural) parses nothing.
            // Later definitions win, mirroring R and the FnTable.
            self.vacuous_helpers.insert(
                name.clone(),
                VacuousHelper {
                    all_span: site.all_span,
                    covered_modes: site.covered_modes.clone(),
                    predicate_name: site.predicate_name.clone(),
                },
            );
        }
    }

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
        let Some(accept) = accept else {
            return;
        };
        // The inline chain first. A helper-call condition -- or an
        // `all()` reduction over a mapped guard-helper result -- only
        // resolves when the inline parse fails; the two shapes are
        // disjoint (a bare call is never an `||` chain).
        if let Some(site) = vacuous_all_site(self, chain, scope) {
            self.arm_vacuous_guard(&site, accept, scope);
            return;
        }
        if matches!(chain, Expr::Call { .. }) {
            if let Some((helper, actual)) = self.vacuous_condition_helper(chain, scope) {
                self.arm_vacuous_helper(&helper, &actual, span_of(chain), accept, scope);
            }
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
            } else if matches!(argument.value, Expr::Call { .. }) {
                if let Some((helper, actual)) =
                    self.vacuous_condition_helper(&argument.value, scope)
                {
                    self.arm_vacuous_helper(
                        &helper,
                        &actual,
                        span_of(&argument.value),
                        accept,
                        scope,
                    );
                }
            }
        }
    }

    /// Resolve an interprocedural guard condition to its (helper, actual)
    /// pair: either a direct helper call `H(v)` over a bare identifier
    /// bound to the helper's formal, or a base `all(V)` reduction over a
    /// local bound by a `map`-family application of a helper
    /// (`valid <- map_lgl(args, H)` proves every element of `args`
    /// passed `H` once the rejection below establishes `all(valid)`).
    /// Anything else (extra call arguments, subset data, a shadowed
    /// `all`, an unrecorded provenance) resolves to no helper.
    fn vacuous_condition_helper(
        &self,
        cond: &Expr,
        scope: &Scope,
    ) -> Option<(VacuousHelper, Expr)> {
        let Expr::Call { func, args, .. } = cond else {
            return None;
        };
        let name = ident_name(func)?;
        let bare = crate::semantic_lists::bare_name(name);
        // The `all(V)` reduction shape.
        if bare == "all"
            && self.resolves_to_base_lenient(name, scope)
            && let [argument] = args.as_slice()
            && argument.name.is_none()
            && let Expr::Ident { name: result, .. } = &argument.value
            && let Some(provenance) = self.vacuous_map_results.get(result)
            && let Some(helper) = self.vacuous_helpers.get(&provenance.helper)
        {
            let actual = Expr::Ident {
                name: provenance.data.clone(),
                span: span_of(&argument.value),
            };
            return Some((helper.clone(), actual));
        }
        // The direct helper-call shape: exactly one actual, bound to the
        // helper's single formal. A bare `H(v)` statement never arms --
        // only guard conditions (`if`, `stopifnot`) reach this hook, so
        // a discarded result cannot validate anything here. A qualified
        // `pkg::H(v)` never resolves to the flat-registered local helper
        // (locals are not called qualified), so only bare spells arm.
        if name.contains("::") || args.len() != 1 || args[0].name.is_some() {
            return None;
        }
        let helper = self.vacuous_helpers.get(name)?;
        // A lexical or shadowing local definition means this call never
        // reaches the registered helper. The one exception is the
        // helper's own top-level definition: pass 3 walks each function
        // body with the name bound to a plain mode-`Function` value
        // (and the project table holds the same entry), so a bare
        // `Function` binding with no alias and no lexical mark is the
        // definition itself, not a shadow. Any other binding of the
        // name in the caller scope voids the helper reading -- the
        // conservative direction for a cross-function fact.
        let shadowed = scope.is_lexical_function(name)
            || scope.function_alias(name).is_some()
            || scope
                .get(name)
                .is_some_and(|ty| !matches!(ty.mode, Mode::Function))
            || !self.fn_table.fns.contains_key(name);
        if shadowed {
            return None;
        }
        let Expr::Ident { .. } = &args[0].value else {
            return None;
        };
        Some((helper.clone(), args[0].value.clone()))
    }

    /// Install one armed guard. The emptiness premise (`x` may be empty
    /// at runtime, open-world widening included) runs here against the
    /// live scope; a proven-empty binding additionally marks the accept
    /// definite through [`vacuous_aggregate_literal`]'s binding proof.
    fn arm_vacuous_guard(&mut self, site: &VacuousAllSite<'_>, accept: Span, scope: &Scope) {
        let definite = vacuous_aggregate_literal(self, site.all_callee, site.all_args, scope)
            == Some(AggregateLiteral::True);
        self.arm_vacuous_guard_parsed(
            site.var,
            site.all_span,
            span_of(site.var),
            &site.covered_modes,
            site.predicate_name.clone(),
            definite,
            accept,
            scope,
        );
    }

    /// Arm the guard of a registered helper over one actual argument
    /// (issue #479). The fact is the helper's; the premise is the
    /// caller's: the recorded type and the emptiness proofs come from
    /// the actual's binding at the call site, so a proven-empty actual
    /// still marks the accept definite. The diagnostic still points at
    /// the helper's `all()` operand (see [`VacuousHelper`]).
    fn arm_vacuous_helper(
        &mut self,
        helper: &VacuousHelper,
        actual: &Expr,
        site_span: Span,
        accept: Span,
        scope: &Scope,
    ) {
        let Expr::Ident { name, .. } = actual else {
            return;
        };
        let definite = !self.test_binding_is_open_world(actual, scope)
            && scope.get(name).is_some_and(|ty| ty.length == Length::Zero);
        self.arm_vacuous_guard_parsed(
            actual,
            helper.all_span,
            site_span,
            &helper.covered_modes,
            helper.predicate_name.clone(),
            definite,
            accept,
            scope,
        );
    }

    /// Shared installer for inline and helper guards: the emptiness
    /// premise against the live scope, the vacuous-mode complement, and
    /// the per-`all()` dedup. Dedup keys on (`all_span`, guarded name,
    /// accepted path): one helper definition may arm distinct guards in
    /// several callers, but one site needs only one guard.
    #[allow(clippy::too_many_arguments)]
    fn arm_vacuous_guard_parsed(
        &mut self,
        var: &Expr,
        all_span: Span,
        site_span: Span,
        covered_modes: &[Mode],
        predicate_name: String,
        definite: bool,
        accept: Span,
        scope: &Scope,
    ) {
        let recorded = scope.get(ident_name(var).unwrap_or_default()).cloned();
        let reference = recorded.clone().unwrap_or_else(RType::unknown);
        if !definite && !self.test_may_be_empty(var, &reference, scope) {
            return;
        }
        let vacuous_modes = vacuous_accept_modes(recorded.as_ref(), covered_modes);
        if vacuous_modes.is_empty() {
            return;
        }
        let Some(var) = ident_name(var).map(str::to_owned) else {
            return;
        };
        // The fixpoint may walk a body more than once; one armed guard
        // per (`all()`, variable, accepted path) is all the diagnostic
        // needs.
        if self
            .vacuous_guards
            .iter()
            .any(|guard| guard.all_span == all_span && guard.var == var && guard.accept == accept)
        {
            return;
        }
        self.vacuous_guards.push(VacuousGuard {
            var,
            all_span,
            site_span,
            accept,
            vacuous_modes,
            recorded: reference,
            predicate_name,
            definite,
            fired: false,
        });
    }

    /// Record `result <- map(data, helper)` provenance (issue #479):
    /// when a `map`-family call applies a registered guard-helper to a
    /// bare-identifier collection, the bound result stands for the
    /// elementwise verdicts, and an `all(result)` rejection later proves
    /// every element of `data` passed the helper. Runs in the pass-3
    /// walk, in source order, so a later rebind overwrites (or drops)
    /// the entry exactly like a guard rebind. A shadowed map callee --
    /// a lexical, project-local, or data-bound same name -- mints no
    /// provenance: only the elementwise verbs qualify.
    pub(crate) fn note_vacuous_map_result(&mut self, target: &Expr, value: &Expr, scope: &Scope) {
        if self.discarding {
            return;
        }
        let Some(result) = ident_name(target) else {
            return;
        };
        let Expr::Call { func, args, .. } = value else {
            self.vacuous_map_results.remove(result);
            return;
        };
        let Some(name) = ident_name(func) else {
            self.vacuous_map_results.remove(result);
            return;
        };
        let bare = crate::semantic_lists::bare_name(name);
        // Matched on the bare callee name; a project-local or lexical
        // shadowing definition disqualifies (checked below), so a user
        // `map_lgl` never mints provenance.
        let in_family = crate::semantic_lists::is_vacuous_map_family(bare);
        // Data first, helper second (`vapply`'s template third): the
        // callback must be a bare identifier naming a registered helper,
        // applied to a bare-identifier collection. Subset data
        // (`args[!is_null]`), anonymous callbacks, and extra actuals
        // keep no provenance -- the demand side only resolves bare
        // identifiers, so anything else could never arm.
        let shape = in_family && map_helper_shape(args);
        let provenance = if shape {
            let Expr::Ident { name: helper, .. } = &args[1].value else {
                unreachable!("shape check matched a bare-identifier callback");
            };
            (self.vacuous_helpers.contains_key(helper.as_str())
                && !shadowed_map_callee(self, name, scope))
            .then(|| {
                ident_name(&args[0].value).map(|data| VacuousMapProvenance {
                    data: data.to_owned(),
                    helper: helper.clone(),
                })
            })
            .flatten()
        } else {
            None
        };
        match provenance {
            Some(provenance) => {
                self.vacuous_map_results
                    .insert(result.to_owned(), provenance);
            }
            None => {
                self.vacuous_map_results.remove(result);
            }
        }
    }

    /// Drop map provenance involving `name`: a reassignment of the
    /// result variable voids its verdicts, and a reassignment of the
    /// data collection means later demands receive a new value. Runs in
    /// the pass-3 walk alongside the guard-rebind drops.
    pub(crate) fn note_vacuous_map_rebind(&mut self, name: &str) {
        if self.discarding {
            return;
        }
        if !self.vacuous_map_results.is_empty() {
            self.vacuous_map_results
                .retain(|result, provenance| result != name && provenance.data != name);
        }
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
            // A named rebind voids map provenance too (issue #479).
            self.note_vacuous_map_rebind(name);
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
        let shadowed = |guard: &VacuousGuard| {
            self.formal_shadows.iter().any(|(formal, function_span)| {
                formal == name
                    && contains(*function_span, *span)
                    && !contains(*function_span, guard.site_span)
            })
        };
        let mut hit: Option<(Span, Mode, String, bool)> = None;
        for guard in &self.vacuous_guards {
            if guard.fired || guard.var != *name || shadowed(guard) {
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

/// Whether the `map`-family callee is shadowed at this site: a lexical
/// function binding, a project-local definition, or any data binding of
/// the bare name means the call never reaches the elementwise verb. An
/// explicit `pkg::` qualification bypasses locals, so only a
/// same-qualified project definition (recorded under the full spelling)
/// disqualifies.
fn shadowed_map_callee(checker: &Checker, name: &str, scope: &Scope) -> bool {
    if name.contains("::") {
        return checker.fn_table.fns.contains_key(name);
    }
    scope.is_lexical_function(name)
        || scope.get(name).is_some()
        || checker.fn_table.fns.contains_key(name)
}

/// Whether a `map`-family call's arguments have the provenance shape:
/// data first, a bare-identifier callback second (`vapply`'s template
/// may follow), all unnamed. Pure syntax, no table reads.
fn map_helper_shape(args: &[Arg]) -> bool {
    let [data, callback, ..] = args else {
        return false;
    };
    data.name.is_none()
        && callback.name.is_none()
        && matches!(data.value, Expr::Ident { .. })
        && matches!(callback.value, Expr::Ident { .. })
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
    fn helper_returned_guards_fire_through_call_sites() {
        // Issue #479: the guard lives in the helper, the demand in the
        // caller. The diagnostic points at the helper's `all()` -- that
        // definition is the defect, as hms's fix showed.
        let diagnostics = check(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nto_seconds <- function(seconds) {\n  if (!is_numeric_or_na(seconds)) stop(\"bad\")\n  sqrt(seconds)\n}\n",
        );
        let hits: Vec<_> = diagnostics.iter().filter(|d| d.code == "RY110").collect();
        assert_eq!(hits.len(), 1, "diagnostics: {diagnostics:?}");
        assert!(
            hits[0].message.contains("fails `is.numeric`"),
            "message must name the helper's predicate: {}",
            hits[0].message
        );
        // Positive helper-call conditions arm the then branch.
        assert!(fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (is_numeric_or_na(v)) sqrt(v)\n}\n"
        ));
        // stopifnot over a helper call guards the continuation.
        assert!(fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  stopifnot(is_numeric_or_na(v))\n  mean(v)\n}\n"
        ));
        // Use-before-def order hides nothing: the registry indexes the
        // file's own top-level definitions before the walk (hms defines
        // `check_args` first).
        assert!(fires(
            "f <- function(v) {\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  sqrt(v)\n}\nis_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\n"
        ));
        // An explicit `return(chain)` body registers the same way.
        // (The parser wraps a bare `return(x)` call in statement
        // position into `Stmt::Return`; the chain sits in its value.)
        assert!(fires(
            "is_numeric_or_na <- function(x) {\n  return(is.numeric(x) || all(is.na(x)))\n}\nf <- function(v) {\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        // A proven-empty actual makes the accept definite.
        assert!(fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nv <- character()\nif (is_numeric_or_na(v)) sqrt(v)\n"
        ));
    }

    #[test]
    fn helper_guards_fire_through_map_lgl_indirection() {
        // Issue #479, the hms args.R form: `check_args` applies the
        // helper elementwise with `map_lgl`, rejects unless every
        // element passed, and the continuation demands the values.
        assert!(fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\ncheck_args <- function(args) {\n  valid <- lapply(args, is_numeric_or_na)\n  if (!all(valid)) stop(\"bad\")\n  sqrt(args)\n}\n"
        ));
        // The positive `all(valid)` form arms the then branch.
        assert!(fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\ncheck_args <- function(args) {\n  valid <- lapply(args, is_numeric_or_na)\n  if (all(valid)) sqrt(args)\n}\n"
        ));
    }

    #[test]
    fn helper_guards_stay_silent_without_validation() {
        // A discarded helper result validates nothing: the continuation
        // is not the accepted path.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  is_numeric_or_na(v)\n  sqrt(v)\n}\n"
        ));
        // A qualified helper call never resolves to the flat-registered
        // local: qualification bypasses locals by design.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!other::is_numeric_or_na(v)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!base::is_numeric_or_na(v)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        // `walk` returns its input, not the verdicts: `all()` over its
        // result tests the data, so no provenance is minted.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(args) {\n  valid <- walk(args, is_numeric_or_na)\n  if (!all(valid)) stop(\"bad\")\n  sqrt(args)\n}\n"
        ));
        // Map verdicts never cross function boundaries, in either
        // definition order: the consumer's `all(valid)` proves nothing
        // about its own `args`.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(args) {\n  valid <- lapply(args, is_numeric_or_na)\n  if (!all(valid)) stop(\"bad\")\n  print(args)\n}\ng <- function(valid, args) {\n  if (!all(valid)) stop(\"bad\")\n  sqrt(args)\n}\n"
        ));
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\ng <- function(valid, args) {\n  if (!all(valid)) stop(\"bad\")\n  sqrt(args)\n}\nf <- function(args) {\n  valid <- lapply(args, is_numeric_or_na)\n  if (!all(valid)) stop(\"bad\")\n  print(args)\n}\n"
        ));
        // A nested closure's verdicts never leak into the enclosing
        // function: `valid` is not even bound in `f`, so its `all()`
        // cannot validate `f`'s `args` (the RY010 on `valid` is a
        // separate, honest diagnostic).
        let nested_leak = check(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(args) {\n  g <- function(args) {\n    valid <- lapply(args, is_numeric_or_na)\n    if (!all(valid)) stop(\"bad\")\n    print(args)\n  }\n  if (!all(valid)) stop(\"bad\")\n  sqrt(args)\n}\n",
        );
        assert!(
            nested_leak.iter().all(|d| d.code != "RY110"),
            "nested verdicts leaked into the parent: {nested_leak:?}"
        );
        // A nested closure still inherits the enclosing verdicts: the
        // `all(valid)` inside `g` reads the same binding `f` validated.
        assert!(fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(args) {\n  valid <- lapply(args, is_numeric_or_na)\n  if (!all(valid)) stop(\"bad\")\n  g <- function() sqrt(args)\n  g()\n}\n"
        ));
        // No downstream mode demand: the gate stays as strict as the
        // inline rule's.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  paste(v, collapse = \",\")\n}\n"
        ));
        // A demand that accepts the vacuous modes is no failure.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  print(v)\n}\n"
        ));
        // The fixed helper form registers nothing: its `all()` is
        // guarded by nonemptiness.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || (length(x) > 0 && all(is.na(x)))\nf <- function(v) {\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        // A multi-formal function is not a guard-helper, even when its
        // body is the chain over the first formal.
        assert!(!fires(
            "helper <- function(x, strict) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!helper(v)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        // A shadowed helper name at the call site never reaches the
        // registered definition.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  is_numeric_or_na <- function(x) TRUE\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        // A shadowed `all` inside the helper body disqualifies it, like
        // the inline rule's shadowed-`all` silence.
        assert!(!fires(
            "all <- function(x) TRUE\nis_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        // Rebinding the actual between guard and demand replaces the
        // guarded value.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!is_numeric_or_na(v)) stop(\"bad\")\n  v <- 1\n  sqrt(v)\n}\n"
        ));
        // Rebinding the mapped collection voids the `all(valid)`
        // verdicts.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\ncheck_args <- function(args) {\n  valid <- lapply(args, is_numeric_or_na)\n  if (!all(valid)) stop(\"bad\")\n  args <- 1\n  sqrt(args)\n}\n"
        ));
        // Extra call arguments do not bind the helper's single formal.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!is_numeric_or_na(v, TRUE)) stop(\"bad\")\n  sqrt(v)\n}\n"
        ));
        // A non-diverging rejection block leaves the accepted path
        // unproven, exactly like the inline rule.
        assert!(!fires(
            "is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))\nf <- function(v) {\n  if (!is_numeric_or_na(v)) v <- NULL\n  sqrt(v)\n}\n"
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
