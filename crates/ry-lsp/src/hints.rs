//! Inlay-hint helpers.

use ry_checker::Scope;
use ry_core::{Expr, SourceFile, Stmt};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::positions::byte_offset_to_position;

/// Collect `InlayHint`s for every assignment whose target is a bare
/// identifier with a known (non-opaque) inferred type. The hint is
/// placed at the end of the identifier name (so the editor renders the
/// ghost text right after the variable, before the `<-`), and its
/// label is the inferred type rendered via `RType`'s `Display` impl
/// (e.g. `: integer<len=1>`).
///
/// The walk recurses into `Stmt::FunctionDef` bodies so that local
/// bindings inside statement-position function literals are annotated
/// too (the top-level scope may or may not track them; if it doesn't,
/// the lookup yields `None` and no hint is emitted).
///
/// Opaque (`Mode::Opaque`) types are deliberately skipped: they
/// represent "we don't know" and would only clutter the editor with
/// unhelpful `: opaque<len=?>?NA?` annotations (the scope's Display
/// string is noisy for opaque modes). For inlay hints, skipping is
/// the better UX.
pub(super) fn collect_inlay_hints(file: &SourceFile, scope: &Scope, text: &str) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    for stmt in &file.stmts {
        collect_inlay_hints_from_stmt(stmt, scope, text, &mut hints);
    }
    hints
}

/// Walk a single statement, appending any inlay hints it contributes
/// to `hints`. Assignments to a bare identifier become hints (when
/// the scope has a non-opaque type for the name); function-definition
/// statements are recursed into so their body bindings are annotated.
fn collect_inlay_hints_from_stmt(
    stmt: &Stmt,
    scope: &Scope,
    text: &str,
    hints: &mut Vec<InlayHint>,
) {
    match stmt {
        // Match only bare-identifier targets in this arm. Complex
        // targets (`df$col <- 1`, `x[1] <- 2`) fall through to
        // the second `Stmt::Assign` arm below and contribute nothing.
        Stmt::Assign {
            target: Expr::Ident { name, span },
            ..
        } => {
            if let Some(t) = scope.get(name) {
                // Opaque types are skipped; see the rationale on
                // `collect_inlay_hints`.
                if matches!(t.mode, ry_core::types::Mode::Opaque) {
                    return;
                }
                // UTF-16 conversion matters here: non-ASCII identifiers
                // must land the hint past their last code unit.
                let pos = byte_offset_to_position(text, span.start + name.len());
                hints.push(InlayHint {
                    position: pos,
                    label: InlayHintLabel::String(format!(": {}", t)),
                    kind: Some(InlayHintKind::TYPE),
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    text_edits: None,
                    data: None,
                });
            }
        }
        // Non-identifier assignment targets (e.g. `x[1] <- 2`,
        // `df$col <- value`) don't introduce a new name in the
        // scope, so they contribute no hints.
        Stmt::Assign { .. } => {}
        // Recurse into anonymous statement-position function literals
        // (a bare `function(...) ...` line) so bindings inside them are
        // annotated too; named functions lower to `Assign` +
        // `Expr::Function` and only their assignment target is hinted.
        Stmt::FunctionDef { body, .. } => {
            for s in body {
                collect_inlay_hints_from_stmt(s, scope, text, hints);
            }
        }
        // Other statement forms (bare expressions, control flow,
        // returns) do not introduce named top-level bindings, so they
        // contribute no hints. We deliberately do NOT recurse into
        // `if`/`for`/`while` bodies here
        // because the top-level scope only tracks the file's top
        // scope; bindings introduced inside control-flow blocks may
        // not be present in `scope`, and emitting a hint for a name
        // the scope doesn't know would be wrong.
        _ => {}
    }
}
