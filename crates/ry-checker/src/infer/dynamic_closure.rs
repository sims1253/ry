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
//! placeholder was analyzed as if it were the final closure. RY010
//! (unbound variable: the alist names) and RY080 (typed-map callback
//! return: a `body<-`-replaced body can return anything) are the two
//! rules whose premise the construction invalidates; everything else
//! (operator type errors and the like) still describes source the
//! author wrote and stays.
//!
//! How much goes opaque depends on which replacement ran:
//! `formals(x) <- v` swaps only the formals list, so the walked body —
//! including closures defined inside it — survives verbatim and their
//! findings stay; only names in the body proper can be alist-installed
//! formals, so only those go quiet. `body(x) <- v` discards the walked
//! body wholesale, and every diagnostic inside it goes with it.
//!
//! Scope matching is lexical and textual: a literal qualifies only when
//! the `formals<-`/`body<-` call names the same bare identifier in the
//! same enclosing statement list (control-flow nesting included, nested
//! function bodies excluded — they are separate scopes that get their
//! own scan). A same-named closure in a different scope, or a
//! construction through a computed receiver (`formals(self$pdf) <-`),
//! is not proven and keeps its diagnostics.

use super::*;

/// Which replacement functions target one placeholder name, and thus
/// how far the placeholder's opacity reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceholderKind {
    /// Only `formals(x) <- v` ran: the formals list (defaults included)
    /// is replaced, but the body and closures defined inside it survive,
    /// so diagnostics inside nested function literals stay reportable.
    FormalsOnly,
    /// `body(x) <- v` ran (with or without a `formals<-`): the walked
    /// body is discarded wholesale, and so are its diagnostics.
    BodyReplaced,
}

/// Names bound by a replacement assignment `formals(x) <- v` /
/// `body(x) <- v` inside one lexical statement list, with the strongest
/// replacement that targets each.
fn replacement_targets(stmts: &[Stmt]) -> FxMap<String, PlaceholderKind> {
    use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
    use std::ops::ControlFlow;

    let mut targets: FxMap<String, PlaceholderKind> = FxMap::default();
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
            if let AstNode::Stmt(Stmt::Assign { target, .. }) = node
                && let Expr::Call { func, args, .. } = target
                && let Some(accessor) = ident_name(func)
                && let Some(argument) = args.first()
                && argument.name.is_none()
                && let Some(name) = ident_name(&argument.value)
            {
                match crate::semantic_lists::bare_name(accessor) {
                    // A later formals<- must not downgrade an earlier
                    // body<-: the body replacement is still in force.
                    "formals" => {
                        targets
                            .entry(name.to_string())
                            .or_insert(PlaceholderKind::FormalsOnly);
                    }
                    "body" => {
                        targets.insert(name.to_string(), PlaceholderKind::BodyReplaced);
                    }
                    _ => {}
                }
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
    targets
}

/// Record the spans of placeholder function literals in one lexical
/// statement list: an `x <- function(...)` (or `x <<-`) whose name later
/// receives `formals<-` / `body<-` in the same list.
fn scope_placeholder_spans(
    stmts: &[Stmt],
    targets: &FxMap<String, PlaceholderKind>,
    out: &mut FxMap<Span, PlaceholderKind>,
) {
    use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
    use std::ops::ControlFlow;

    if targets.is_empty() {
        return;
    }
    let _ = walk_stmts(
        stmts,
        Walk {
            assign_targets: false,
            assign_operands: true,
            dollar_args: false,
            fn_bodies: false,
            control_tests: true,
        },
        |node, _: usize| -> ControlFlow<(), Descend> {
            if let AstNode::Stmt(Stmt::Assign { target, value, .. }) = node
                && let Some(name) = binding_name(target)
                && let Some(kind) = targets.get(name)
                && let Expr::Function { span, .. } = value
            {
                out.insert(*span, *kind);
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
/// function body recursively (a body's `formals<-` targets cannot reach
/// an outer scope's same-named closure, and an outer target cannot reach
/// into a nested body). `emit_diagnostics` runs this before the pass-3
/// walk, alongside the other pre-indexed facts; `enter_function_body`
/// consults the map when it walks a literal.
pub(crate) fn index_dynamic_closure_literals(
    stmts: &[Stmt],
    out: &mut FxMap<Span, PlaceholderKind>,
) {
    let targets = replacement_targets(stmts);
    scope_placeholder_spans(stmts, &targets, out);
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
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}
