//! Dynamically constructed closures: `formals(f) <- ...` /
//! `body(f) <- ...` placeholders (issue #380).
//!
//! R packages build closures ry's static view never sees complete.
//! distr6's method templates assign a placeholder literal first and then
//! rewrite it through the replacement-function machinery:
//!
//! ```r
//! trafo <- function() {
//!   return(x)                    # x is not bound *here*
//! }
//! formals(trafo) <- alist(x = )  # ...but the alist installs x
//! ```
//!
//! After `formals<-` runs, the closure's formals (and with `body<-` its
//! body) are whatever the construction code supplied, so a name the
//! placeholder body references may be bound in every call R ever makes
//! while being absent from every formals list ry analyzed. Distr6's
//! `genExp` default `trafo` and `makeChecks` assertion builders are the
//! corpus instances; the 17 audit findings on that shape were all false
//! positives, and the construction site itself is clean R.
//!
//! The design choice is opacity over simulation (the issue's preferred
//! direction): rather than trying to reconstruct the constructed
//! closure's real formals from the `alist()` source — the value can be
//! computed (`c(formals(f), list(self = self))`), conditionally built,
//! or taken from a runtime object — the checker marks the placeholder
//! literal's span and drops the diagnostics that only exist because the
//! placeholder was analyzed as if it were the final closure. Everything
//! else (operator type errors and the like) still describes source the
//! author wrote and stays.
//!
//! How much goes opaque depends on which replacement ran:
//! `formals(x) <- v` swaps only the formals list, so the walked body —
//! including closures defined inside it — survives verbatim and their
//! findings stay; only names in the body proper can be alist-installed
//! formals, so only those RY010s go quiet. `body(x) <- v` discards the
//! walked body wholesale, so both the RY010s and the RY080s inside it
//! go with it (a `body<-`-replaced callback can return anything). A
//! typed-map error in a `formals<-`-only body is genuine — RY080
//! anchors at the `map_*` call and survives.
//!
//! Scope matching is lexical and source-ordered: a replacement marks
//! only the function literal bound to that name *at that point* in the
//! same enclosing statement list. A rebind (`f <- anything`) ends the
//! association, so a later same-named literal keeps its diagnostics
//! exactly as R's copy-on-replace semantics demand, and a replacement
//! with no preceding literal marks nothing. Control-flow nesting is
//! included, nested function bodies are excluded (they are separate
//! scopes that get their own scan), and `local({...})` argument blocks
//! are separate runtime scopes in both directions: a replacement inside
//! one cannot reach an outer literal, and the block's own literals pair
//! with the block's own replacements. A construction through a computed
//! receiver (`formals(self$pdf) <-`) is not proven and keeps its
//! diagnostics.
//!
//! Known misses, accepted as over-approximation: a conditional
//! replacement (`if (c) formals(f) <- ...`, `c` false at runtime)
//! still grants opacity to the preceding literal; `eval`-style
//! argument blocks other than `local()` are not boundary-tracked; and
//! call sites that pass a constructed closure onward — a by-name
//! `map_dbl(x, f)` callback, or arguments matched against recorded
//! formals (RY090/RY091) — are still judged by the placeholder's
//! recorded returns and formals even under `body<-`.

use super::*;

/// Which replacement functions target one placeholder name, and thus
/// how far the placeholder's opacity reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceholderKind {
    /// Only `formals(x) <- v` ran: the formals list (defaults included)
    /// is replaced, but the body and closures defined inside it survive,
    /// so diagnostics inside nested function literals stay reportable
    /// and RY080 is not dropped at all.
    FormalsOnly,
    /// `body(x) <- v` ran (with or without a `formals<-`): the walked
    /// body is discarded wholesale, and so are its RY010/RY080
    /// diagnostics.
    BodyReplaced,
}

/// Whether `func` is a bare (possibly `pkg::`-qualified) `local` call
/// head: `local({...})` evaluates its block in a fresh environment, so
/// the enclosing scope's placeholder scan must not cross it.
fn is_local_call(func: &Expr) -> bool {
    ident_name(func).is_some_and(|name| crate::semantic_lists::bare_name(name) == "local")
}

/// Record the spans of placeholder function literals in one lexical
/// statement list, in source order: a replacement assignment
/// `formals(x) <- v` / `body(x) <- v` marks the function literal
/// currently bound to `x`, and any rebind of `x` ends the association.
fn scope_placeholder_spans(stmts: &[Stmt], out: &mut FxMap<Span, PlaceholderKind>) {
    use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
    use std::ops::ControlFlow;

    // The function literal currently bound to each name in this scope:
    // the only literal a replacement can act on at this point in the
    // source order. `formals(f) <-` after `f <- g` modifies g's closure,
    // not some earlier literal, so any rebind (to a literal or not)
    // replaces the entry.
    let mut current: FxMap<String, Span> = FxMap::default();
    let _ = walk_stmts(
        stmts,
        Walk {
            // Replacement targets are not evaluated as calls; reading
            // them off the Assign statement below is exact.
            assign_targets: false,
            // Defaults and other operand expressions evaluate in this
            // scope, so nested blocks stay visible.
            assign_operands: true,
            dollar_args: false,
            // A nested closure is a different scope: its own entry scan
            // covers its body.
            fn_bodies: false,
            control_tests: true,
        },
        |node, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Stmt(Stmt::Assign { target, value, .. }) => {
                    if let Expr::Call { func, args, .. } = target
                        && let Some(accessor) = ident_name(func)
                        && let Some(argument) = args.first()
                        && argument.name.is_none()
                        && let Some(name) = ident_name(&argument.value)
                    {
                        match crate::semantic_lists::bare_name(accessor) {
                            // A later formals<- must not downgrade an
                            // earlier body<-: the body replacement is
                            // still in force.
                            "formals" => {
                                if let Some(&span) = current.get(name) {
                                    out.entry(span).or_insert(PlaceholderKind::FormalsOnly);
                                }
                            }
                            "body" => {
                                if let Some(&span) = current.get(name) {
                                    out.insert(span, PlaceholderKind::BodyReplaced);
                                }
                            }
                            _ => {}
                        }
                    } else if let Some(name) = binding_name(target) {
                        if let Expr::Function { span, .. } = value {
                            current.insert(name.to_string(), *span);
                        } else {
                            current.remove(name);
                        }
                    }
                }
                // local({...}) runs its block in a fresh environment: a
                // replacement inside cannot touch this scope's bindings
                // and this scope's literals are invisible inside. The
                // recursion in `index_dynamic_closure_literals` indexes
                // the block as its own scope.
                AstNode::Expr(Expr::Call { func, .. }) if is_local_call(func) => {
                    return ControlFlow::Continue(Descend::Skip);
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}

/// Spans of every function literal lexically nested inside a
/// placeholder body, at any depth. A `formals<-` leaves these closures
/// in place, so their diagnostics must survive the placeholder's
/// opacity filter; containment is decided on spans because the nested
/// walk has already finished by the time the filter runs.
pub(crate) fn nested_function_literal_spans(body: &[Stmt]) -> Vec<Span> {
    use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
    use std::ops::ControlFlow;

    let mut spans = Vec::new();
    let _ = walk_stmts(
        body,
        Walk::ALL,
        |node, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Expr(Expr::Function { span, .. }) => spans.push(*span),
                AstNode::Stmt(Stmt::FunctionDef { span, .. }) => spans.push(*span),
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
    spans
}

/// Index every placeholder function-literal span in the file, with the
/// strongest replacement targeting it.
///
/// One scan per lexical scope: the top level first, then each nested
/// function body and each `local({...})` argument block recursively (a
/// body's `formals<-` targets cannot reach an outer scope's same-named
/// closure, and an outer target cannot reach into a nested body).
/// `emit_diagnostics` runs this before the pass-3 walk, alongside the
/// other pre-indexed facts; `enter_function_body` consults the map when
/// it walks a literal.
pub(crate) fn index_dynamic_closure_literals(
    stmts: &[Stmt],
    out: &mut FxMap<Span, PlaceholderKind>,
) {
    scope_placeholder_spans(stmts, out);
    // Nested scopes: every function literal or definition body found
    // anywhere in this list gets its own target scan. `fn_bodies` stays
    // off here only to avoid re-scanning a body twice (the recursion
    // below reaches it exactly once); statement nesting still descends.
    use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
    use std::ops::ControlFlow;
    let _ = walk_stmts(
        stmts,
        Walk {
            assign_targets: true,
            assign_operands: true,
            dollar_args: false,
            fn_bodies: false,
            control_tests: true,
        },
        |node, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Expr(Expr::Function { body, .. }) => {
                    index_dynamic_closure_literals(body, out);
                }
                AstNode::Stmt(Stmt::FunctionDef { body, .. }) => {
                    index_dynamic_closure_literals(body, out);
                }
                // A local() block is a runtime scope of its own: index
                // its brace arguments separately instead of letting the
                // walk descend (which would double-index them).
                AstNode::Expr(Expr::Call { func, args, .. }) if is_local_call(func) => {
                    for argument in args {
                        if let Expr::Block { body, .. } = &argument.value {
                            index_dynamic_closure_literals(body, out);
                        }
                    }
                    return ControlFlow::Continue(Descend::Skip);
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}
