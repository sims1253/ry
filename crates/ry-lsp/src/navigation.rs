//! Go-to-definition and references helpers.
//!
//! These walk the AST to find definition sites and references of an
//! identifier. They are pure functions over the parsed `SourceFile` and
//! source text; the `Backend` request handlers call them after resolving
//! the identifier under the cursor.

use ry_core::{Expr, SourceFile, Span, Stmt};
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::util::byte_offset_to_position;

/// Find every definition site of `name` in `file`, returning each as an
/// LSP `Location` inside `uri`.
pub(super) fn find_definition_locations(
    file: &SourceFile,
    name: &str,
    uri: &Url,
    text: &str,
) -> Vec<Location> {
    let mut spans: Vec<Span> = Vec::new();
    for stmt in &file.stmts {
        find_def_spans_in_stmt(stmt, name, &mut spans);
    }
    spans
        .into_iter()
        .map(|sp| span_to_location(sp, name, uri, text))
        .collect()
}

/// Convert a definition-site `Span` into an LSP `Location`. The range
/// highlights the identifier itself (start .. start + name.len()).
/// Columns are UTF-16 code units, converted from the span's byte
/// offsets against the source text.
fn span_to_location(span: Span, name: &str, uri: &Url, text: &str) -> Location {
    let start = byte_offset_to_position(text, span.start);
    let end = byte_offset_to_position(text, span.start + name.len());
    Location {
        uri: uri.clone(),
        range: Range { start, end },
    }
}

fn find_def_spans_in_stmt(stmt: &Stmt, name: &str, out: &mut Vec<Span>) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            if let Expr::Ident { name: n, span } = target {
                if n == name {
                    out.push(*span);
                }
            }
            find_def_spans_in_expr(value, name, out);
        }
        Stmt::FunctionDef {
            name: fn_name,
            body,
            span,
            ..
        } => {
            if let Some(n) = fn_name {
                if n == name {
                    out.push(*span);
                }
            }
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Stmt::If { then, else_, .. } => {
            for s in then {
                find_def_spans_in_stmt(s, name, out);
            }
            if let Some(else_block) = else_ {
                for s in else_block {
                    find_def_spans_in_stmt(s, name, out);
                }
            }
        }
        Stmt::For {
            name: loop_var,
            body,
            name_span,
            ..
        } => {
            if loop_var == name {
                out.push(*name_span);
            }
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Stmt::While { body, .. } => {
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                find_def_spans_in_expr(v, name, out);
            }
        }
        Stmt::Expr(e) => find_def_spans_in_expr(e, name, out),
    }
}

fn find_def_spans_in_expr(expr: &Expr, name: &str, out: &mut Vec<Span>) {
    match expr {
        Expr::Function { body, .. } => {
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Expr::Block { body, .. } => {
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Expr::If { then, else_, .. } => {
            find_def_spans_in_expr(then, name, out);
            if let Some(e) = else_ {
                find_def_spans_in_expr(e, name, out);
            }
        }
        Expr::Call { func, args, .. } => {
            find_def_spans_in_expr(func, name, out);
            for arg in args {
                find_def_spans_in_expr(&arg.value, name, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            find_def_spans_in_expr(lhs, name, out);
            find_def_spans_in_expr(rhs, name, out);
        }
        Expr::UnaryOp { expr, .. } => find_def_spans_in_expr(expr, name, out),
        Expr::Index { base, args, .. } => {
            find_def_spans_in_expr(base, name, out);
            for arg in args {
                find_def_spans_in_expr(&arg.value, name, out);
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

/// Walk the AST of `file` collecting every reference to `name` as an LSP
/// `Location` inside `uri`. When `include_declaration` is true,
/// definition sites are included alongside plain references.
pub(super) fn find_references_in_file(
    file: &SourceFile,
    name: &str,
    uri: &Url,
    text: &str,
    include_declaration: bool,
) -> Vec<Location> {
    let mut spans: Vec<Span> = Vec::new();
    for stmt in &file.stmts {
        find_ref_spans_in_stmt(stmt, name, &mut spans, include_declaration);
    }
    let mut locations = Vec::with_capacity(spans.len());
    for span in spans {
        let start = byte_offset_to_position(text, span.start);
        let end = byte_offset_to_position(text, span.end);
        let end = if start == end {
            Position {
                line: start.line,
                character: start.character + 1,
            }
        } else {
            end
        };
        locations.push(Location {
            uri: uri.clone(),
            range: Range { start, end },
        });
    }
    locations
}

fn find_ref_spans_in_stmt(stmt: &Stmt, name: &str, out: &mut Vec<Span>, include_declaration: bool) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            if include_declaration {
                if let Expr::Ident { name: n, span } = target {
                    if n == name {
                        out.push(*span);
                    }
                }
            }
            find_ref_spans_in_expr(value, name, out, include_declaration);
        }
        Stmt::FunctionDef {
            name: fn_name,
            body,
            span,
            ..
        } => {
            if include_declaration {
                if let Some(n) = fn_name {
                    if n == name {
                        out.push(*span);
                    }
                }
            }
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            find_ref_spans_in_expr(cond, name, out, include_declaration);
            for s in then {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
            if let Some(else_block) = else_ {
                for s in else_block {
                    find_ref_spans_in_stmt(s, name, out, include_declaration);
                }
            }
        }
        Stmt::For {
            name: loop_var,
            iter,
            body,
            name_span,
            ..
        } => {
            if include_declaration && loop_var == name {
                out.push(*name_span);
            }
            find_ref_spans_in_expr(iter, name, out, include_declaration);
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Stmt::While { cond, body, .. } => {
            find_ref_spans_in_expr(cond, name, out, include_declaration);
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                find_ref_spans_in_expr(v, name, out, include_declaration);
            }
        }
        Stmt::Expr(e) => find_ref_spans_in_expr(e, name, out, include_declaration),
    }
}

fn find_ref_spans_in_expr(expr: &Expr, name: &str, out: &mut Vec<Span>, include_declaration: bool) {
    match expr {
        Expr::Ident { name: n, span } => {
            if n == name {
                out.push(*span);
            }
        }
        Expr::Call { func, args, .. } => {
            find_ref_spans_in_expr(func, name, out, include_declaration);
            for arg in args {
                find_ref_spans_in_expr(&arg.value, name, out, include_declaration);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            find_ref_spans_in_expr(lhs, name, out, include_declaration);
            find_ref_spans_in_expr(rhs, name, out, include_declaration);
        }
        Expr::UnaryOp { expr, .. } => find_ref_spans_in_expr(expr, name, out, include_declaration),
        Expr::Index { base, args, .. } => {
            find_ref_spans_in_expr(base, name, out, include_declaration);
            for arg in args {
                find_ref_spans_in_expr(&arg.value, name, out, include_declaration);
            }
        }
        Expr::Function { body, .. } => {
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Expr::Block { body, .. } => {
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            find_ref_spans_in_expr(cond, name, out, include_declaration);
            find_ref_spans_in_expr(then, name, out, include_declaration);
            if let Some(e) = else_ {
                find_ref_spans_in_expr(e, name, out, include_declaration);
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => {}
    }
}
