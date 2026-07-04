//! Folding-range computation and shared AST-span helpers (PLAN Phase E3).
//!
//! Collects `FoldingRange`s for every multi-line foldable block, and
//! exposes `span_of_stmt` / `span_of_expr` (also used by the
//! selection-range handler).

use ry_core::{Expr, SourceFile, Span, Stmt};
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};

use crate::util::byte_offset_to_position;

/// Collect `FoldingRange`s for every multi-line foldable block in the
/// file. R's foldable regions are function bodies, `if`/`else` blocks,
/// `for`/`while` loop bodies, and multi-line assignment RHS. A region is
/// foldable only when its `Span` crosses a newline.
pub(super) fn collect_folding_ranges(file: &SourceFile, text: &str) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    for stmt in &file.stmts {
        collect_folding_from_stmt(stmt, text, &mut ranges);
    }
    ranges
}

/// Walk a single statement, appending any foldable region it
/// contributes to `ranges`, then recurse into nested blocks.
fn collect_folding_from_stmt(stmt: &Stmt, text: &str, ranges: &mut Vec<FoldingRange>) {
    if let Some(span) = span_of_stmt(stmt) {
        let start_line = span.line as u32;
        let end_line = byte_offset_to_position(text, span.end).line;
        if end_line > start_line {
            ranges.push(FoldingRange {
                start_line,
                end_line,
                start_character: None,
                end_character: None,
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            });
        }
    }
    match stmt {
        Stmt::FunctionDef { body, .. } | Stmt::For { body, .. } | Stmt::While { body, .. } => {
            for s in body {
                collect_folding_from_stmt(s, text, ranges);
            }
        }
        Stmt::If { then, else_, .. } => {
            for s in then {
                collect_folding_from_stmt(s, text, ranges);
            }
            if let Some(e) = else_ {
                for s in e {
                    collect_folding_from_stmt(s, text, ranges);
                }
            }
        }
        // An `Assign` may carry a multi-line function literal on its
        // RHS (the common `f <- function() { ... }` pattern).
        Stmt::Assign { value, .. } => {
            collect_folding_from_expr(value, text, ranges);
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                collect_folding_from_expr(v, text, ranges);
            }
        }
        Stmt::Expr(e) => collect_folding_from_expr(e, text, ranges),
    }
}

/// Recurse into an expression looking for nested multi-line blocks.
fn collect_folding_from_expr(expr: &Expr, text: &str, ranges: &mut Vec<FoldingRange>) {
    match expr {
        Expr::Function { body, span, .. } => {
            push_range_if_multiline(*span, text, ranges);
            for s in body {
                collect_folding_from_stmt(s, text, ranges);
            }
        }
        Expr::If {
            cond,
            then,
            else_,
            span,
        } => {
            push_range_if_multiline(*span, text, ranges);
            collect_folding_from_expr(cond, text, ranges);
            collect_folding_from_expr(then, text, ranges);
            if let Some(e) = else_ {
                collect_folding_from_expr(e, text, ranges);
            }
        }
        Expr::Call {
            func, args, span, ..
        } => {
            push_range_if_multiline(*span, text, ranges);
            collect_folding_from_expr(func, text, ranges);
            for arg in args {
                collect_folding_from_expr(&arg.value, text, ranges);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_folding_from_expr(lhs, text, ranges);
            collect_folding_from_expr(rhs, text, ranges);
        }
        Expr::UnaryOp { expr, .. } => collect_folding_from_expr(expr, text, ranges),
        Expr::Index { base, args, .. } => {
            collect_folding_from_expr(base, text, ranges);
            for arg in args {
                collect_folding_from_expr(&arg.value, text, ranges);
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Ident { .. }
        | Expr::Unknown(_) => {}
    }
}

/// Push a folding range for `span` when its end lands on a later line
/// than its start.
fn push_range_if_multiline(span: Span, text: &str, ranges: &mut Vec<FoldingRange>) {
    let start_line = span.line as u32;
    let end_line = byte_offset_to_position(text, span.end).line;
    if end_line > start_line {
        ranges.push(FoldingRange {
            start_line,
            end_line,
            start_character: None,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        });
    }
}

/// Return the `Span` of a statement, if it carries one. Shared with the
/// selection-range handler.
pub(super) fn span_of_stmt(stmt: &Stmt) -> Option<Span> {
    match stmt {
        Stmt::Assign { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::FunctionDef { span, .. }
        | Stmt::Return { span, .. } => Some(*span),
        Stmt::Expr(e) => span_of_expr(e),
    }
}

/// Return the `Span` of an expression. Shared with the selection-range
/// handler.
pub(super) fn span_of_expr(expr: &Expr) -> Option<Span> {
    match expr {
        Expr::Logical(_, s)
        | Expr::Integer(_, s)
        | Expr::Double(_, s)
        | Expr::String(_, s)
        | Expr::Null(s)
        | Expr::Na(_, s)
        | Expr::Call { span: s, .. }
        | Expr::Ident { span: s, .. }
        | Expr::BinOp { span: s, .. }
        | Expr::UnaryOp { span: s, .. }
        | Expr::Index { span: s, .. }
        | Expr::Function { span: s, .. }
        | Expr::If { span: s, .. }
        | Expr::Unknown(s) => Some(*s),
    }
}
