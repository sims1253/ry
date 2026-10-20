//! Vacuous `all()` in validation guards (RY110).
//!
//! In R, `all(logical(0))` is `TRUE`. A guard such as
//! `is.numeric(x) || all(is.na(x))` therefore accepts any zero-length
//! input: `character()` fails `is.numeric` but passes `all(is.na(x))`.
//! The value then fails downstream, in code that assumes the predicate's
//! mode.
//!
//! An `||` chain that combines a mode predicate over `x` with a bare
//! `all(is.na(x))` arms a [`VacuousGuard`] on the accepted path. The
//! diagnostic fires only when an empty value that fails the predicate
//! reaches a parameter `type` declared by a typeshed stub (the
//! declarations RY092 checks). Without that demand gate the rule would
//! flag every permissive guard, and `all()` over empty is often fine.
//!
//! A single-formal function in the checked file whose body is the chain
//! registers a [`VacuousHelper`]. Calls that pass a value to it arm the
//! guard: `if (H(v))`, `stopifnot(H(v))`, or `all()` over a `map`-family
//! result. The demand must sit in the calling function. A helper in
//! another file never arms, because the diagnostic would point at a byte
//! range in a foreign file. A hop from a validating function to a
//! demanding one stays silent too.

use super::*;

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

/// One armed guard awaiting its downstream demand. The first
/// stub-declared parameter type inside `accept` that rejects one of
/// `vacuous_modes` for `var` fires RY110 at `all_span`.
pub(crate) struct VacuousGuard {
    var: String,
    all_span: Span,
    /// The guard condition's span in the guarding function. The shadow
    /// check reads it: a nested formal shadows the name only when it
    /// contains the demand but not this site. For a helper guard this is
    /// the caller's condition, while `all_span` points into the helper.
    site_span: Span,
    /// Byte range of the accepted path.
    accept: Span,
    /// The empty modes the guard admits vacuously. NULL is excluded: a
    /// dispatching demand may accept a zero-length NULL, and arithmetic
    /// recycles it.
    vacuous_modes: Vec<Mode>,
    /// The binding's recorded type at the guard, for RY092's dispatch
    /// check at demand sites.
    recorded: RType,
    predicate_name: String,
    /// The guarded binding is proven empty, so `all()` is TRUE for
    /// certain.
    definite: bool,
    fired: bool,
}

/// A single-formal function whose body is (or returns) the vacuous-all
/// chain over that formal, such as
/// `is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))`.
/// Call sites arm an ordinary [`VacuousGuard`] over the actual argument.
/// The diagnostic points at the helper's `all()` operand, because the
/// helper definition is the defect.
#[derive(Debug, Clone)]
pub(crate) struct VacuousHelper {
    all_span: Span,
    covered_modes: Vec<Mode>,
    predicate_name: String,
}

/// `result` is bound by a `map`-family call that applies a guard-helper
/// to `data` (`valid <- map_lgl(args, is_numeric_or_na)`). An
/// `all(result)` guard then proves every element of `data` passed the
/// helper.
#[derive(Debug, Clone)]
pub(crate) struct VacuousMapProvenance {
    data: String,
    helper: String,
}

/// One parsed `P(x) || all(is.na(x))` chain.
struct VacuousAllSite<'a> {
    var: &'a Expr,
    all_span: Span,
    /// The `is.na(x)` argument of the `all()` operand.
    all_arg: &'a Expr,
    /// Modes the chain's predicates over `var` cover.
    covered_modes: Vec<Mode>,
    /// The first covering predicate's bare name, for the message.
    predicate_name: String,
}

/// Parse an `||` chain into the vacuous-all guard shape: at least one
/// bare base `all(is.na(x))` operand and at least one positive base mode
/// predicate over the same `x`. The predicate marks a validation guard,
/// and keeping it on `x` keeps the message and suggested rewrite about
/// `x`. Other operands are ignored. The fixed form
/// `length(x) > 0 && all(is.na(x))` is a `&&` operand, so it stays silent.
fn vacuous_all_site<'a>(
    checker: &Checker,
    chain: &'a Expr,
    scope: &Scope,
) -> Option<VacuousAllSite<'a>> {
    let mut leaves = Vec::new();
    collect_or_leaves(chain, &mut leaves);
    let mut all_leaf: Option<(&Expr, Span, &Expr)> = None;
    for leaf in &leaves {
        let Expr::Call { func, args, span } = leaf else {
            continue;
        };
        let Some(name) = ident_name(func) else {
            continue;
        };
        if crate::semantic_lists::bare_name(name) == "all" {
            // `any(is.na(x))` is FALSE over empty, so only `all` accepts.
            let var = bare_all_is_na_var(checker, name, args, scope)?;
            match all_leaf {
                Some((first, _, _)) if !same_ident(first, var) => return None,
                Some(_) => {}
                None => all_leaf = Some((var, *span, &args[0].value)),
            }
        }
    }
    let (var, all_span, all_arg) = all_leaf?;
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
        // Class predicates (`is.data.frame`, `inherits`) make a class
        // claim the mode lattice cannot complement, so they do not cover.
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
        all_arg,
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
/// with one unnamed argument, which is base `is.na` over one identifier.
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
/// the lattice can enumerate. Group predicates (`is.numeric`) carry a
/// union.
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

/// The empty modes the guard admits vacuously: modes that fail every
/// covering predicate and are plausible for the recorded type. A known
/// mode or union restricts plausibility to its own modes. Opaque or
/// unbound values keep every mode.
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
    let modes = recorded.map(super::types::modes_of);
    let plausible: Vec<Mode> = match modes {
        Some(modes) if !modes.iter().any(|mode| mode.is_unknown_or_non_vector()) => modes,
        _ => pool.to_vec(),
    };
    plausible
        .into_iter()
        .filter(|mode| !covered.contains(mode))
        .collect()
}

impl Checker {
    /// Register every top-level single-formal function in the checked
    /// file whose body is (or returns) the vacuous-all chain over that
    /// formal. Runs once before the pass-3 walk, so a call before the
    /// helper's definition still arms. The registry is file-local, so a
    /// diagnostic never carries another file's byte span. The parse uses
    /// an empty scope with the populated tables, so a top-level
    /// `all <- function...` disqualifies the helper.
    pub(crate) fn index_vacuous_helpers(&mut self, stmts: &[Stmt]) {
        use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
        use std::ops::ControlFlow;
        // Enter `if`/`for`/`while` bodies but not expressions: a helper
        // bound in a top-level branch registers, a nested closure does not.
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
            // The body is the chain or returns it. The parser lowers a
            // braced `return(chain)` to `Stmt::Expr(Call)`, so unwrap that
            // spelling too.
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
            // Only a chain over the helper's own formal validates the
            // passed value.
            if ident_name(site.var) != Some(param.name.as_str()) {
                continue;
            }
            // Later definitions win, as in R.
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

    /// Arm a vacuous-all guard in an `if` condition. The accepted path is
    /// the `then` branch for a positive condition. For a negated one it
    /// is the `else` branch, or the continuation after the `if` when the
    /// rejection block (`stop()`, `return()`) provably diverges.
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
            // The continuation after a positive guard also runs when the
            // guard is FALSE, so only `then` is armed.
            stmt_list_span(then)
        };
        let Some(accept) = accept else {
            return;
        };
        self.arm_vacuous_condition(chain, accept, scope);
    }

    /// Arm each `stopifnot(...)` argument as a guard over the
    /// continuation of the enclosing statement list. `stopifnot` itself
    /// rejects, so no divergence proof is needed.
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
            self.arm_vacuous_condition(&argument.value, accept, scope);
        }
    }

    /// Arm one guard condition: an inline `||` chain, or else a
    /// helper-call condition. The shapes are disjoint because a bare call
    /// is never an `||` chain.
    fn arm_vacuous_condition(&mut self, cond: &Expr, accept: Span, scope: &Scope) {
        if let Some(site) = vacuous_all_site(self, cond, scope) {
            self.arm_vacuous_guard(&site, accept, scope);
        } else if let Some((helper, actual)) = self.vacuous_condition_helper(cond, scope) {
            self.arm_vacuous_helper(&helper, &actual, span_of(cond), accept, scope);
        }
    }

    /// Resolve a guard condition to its (helper, actual) pair: a direct
    /// helper call `H(v)` over a bare identifier, or a base `all(V)` over
    /// a local bound by a `map`-family application of a helper.
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
        // A qualified `pkg::H(v)` bypasses the local helper.
        if name.contains("::") || args.len() != 1 || args[0].name.is_some() {
            return None;
        }
        let helper = self.vacuous_helpers.get(name)?;
        // Any local binding of the name shadows the helper, except the
        // plain mode-`Function` binding pass 3 gives the helper's own
        // top-level definition.
        let shadowed = scope.is_lexical_function(name)
            || scope.function_alias(name).is_some()
            || scope
                .get(name)
                .is_some_and(|ty| !matches!(ty.mode, Mode::Function))
            || !self.fn_table.functions.binds(name);
        if shadowed {
            return None;
        }
        let Expr::Ident { .. } = &args[0].value else {
            return None;
        };
        Some((helper.clone(), args[0].value.clone()))
    }

    /// Install one armed guard. A proven-empty local (not open-world)
    /// makes the accept definite: `is.na` keeps `x`'s length, so `all()`
    /// sees an empty vector.
    fn arm_vacuous_guard(&mut self, site: &VacuousAllSite<'_>, accept: Span, scope: &Scope) {
        let definite = sole_is_na_ident(site.all_arg).is_some_and(|var| {
            !self.test_binding_is_open_world(var, scope)
                && scope
                    .get(ident_name(var).unwrap_or_default())
                    .is_some_and(|ty| ty.length == Length::Zero)
        });
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

    /// Arm a registered helper's guard over one actual argument. The
    /// recorded type and emptiness proofs come from the actual's binding
    /// at the call site.
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

    /// Install an inline or helper guard when `var` may be empty and some
    /// mode is admitted vacuously.
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
        // The fixpoint may walk a body more than once.
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

    /// Record `result <- map(data, helper)` provenance when a
    /// `map`-family call applies a registered guard-helper to a
    /// bare-identifier collection. Any other value for `result` drops its
    /// entry. A shadowed map callee records nothing.
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
        let in_family = crate::semantic_lists::is_vacuous_map_family(bare);
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

    /// Drop map provenance whose result or data is `name`.
    pub(crate) fn note_vacuous_map_rebind(&mut self, name: &str) {
        if self.discarding {
            return;
        }
        if !self.vacuous_map_results.is_empty() {
            self.vacuous_map_results
                .retain(|result, provenance| result != name && provenance.data != name);
        }
    }

    /// Drop every armed guard over `name`, because a reassignment replaces
    /// the guarded value. The walk runs in source order, so earlier
    /// demands keep their guards.
    pub(crate) fn note_vacuous_guard_rebind(&mut self, name: &str) {
        if !self.vacuous_guards.is_empty() {
            self.vacuous_guards.retain(|guard| guard.var != name);
        }
    }

    /// `assign("x", v)` rebinds `x` like `x <- v`.
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
            self.note_vacuous_map_rebind(name);
        }
    }

    /// The demand half of RY110, called for every argument whose
    /// parameter has a stub-declared type. Fires when the argument is the
    /// guarded variable on the accepted path, no nested formal shadows
    /// it, and the declared type rejects a vacuous mode.
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
        // A nested formal that contains the demand but not the guard is a
        // different binding. `vapply` over empty never calls its lambda.
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
            // A generic demand may dispatch a classed or NULL value to a
            // method that accepts it.
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
        // The Math group errors on an empty non-numeric value, while
        // `mean()` warns and returns NA. "cannot use" covers both.
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

/// Whether a local or project binding shadows the `map`-family callee.
/// A `pkg::` qualification bypasses locals.
fn shadowed_map_callee(checker: &Checker, name: &str, scope: &Scope) -> bool {
    if name.contains("::") {
        return checker.fn_table.functions.binds(name);
    }
    scope.is_lexical_function(name)
        || scope.get(name).is_some()
        || checker.fn_table.functions.binds(name)
}

/// Data first and a bare-identifier callback second, both unnamed.
/// `vapply`'s template may follow.
fn map_helper_shape(args: &[Arg]) -> bool {
    let [data, callback, ..] = args else {
        return false;
    };
    data.name.is_none()
        && callback.name.is_none()
        && matches!(data.value, Expr::Ident { .. })
        && matches!(callback.value, Expr::Ident { .. })
}

/// The root identifier of an assignment target. `x[1] <- v` can coerce
/// the whole vector's mode, so an index target counts as a rebind of
/// its root.
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

/// Index, in one walk, the byte range of the statements that follow each
/// statement in its list (the accepted path of a rejecting guard), and
/// each function's formal names with the function's span (the shadow
/// set).
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

/// Statement lists nested inside expressions. Each function body gets
/// its own continuation ranges, so a guard at the end of one function
/// cannot reach the next definition.
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

    #[test]
    fn fires_on_the_hms_shape_with_a_downstream_demand() {
        // The guard accepts `character()` vacuously and the demand
        // rejects it.
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
        // The fixed form: nonemptiness guards the `all()`, so the operand
        // is not a bare `all(is.na(x))`.
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
        // The guard lives in the helper, the demand in the caller. The
        // diagnostic points at the helper's `all()`.
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
        // A call before the helper's definition still arms.
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
        // `check_args` applies the helper with `map_lgl`, rejects unless
        // every element passed, and the continuation demands the values.
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
