//! Go-to-definition, references, document-highlight, and rename helpers
//! (PLAN Phase E3).
//!
//! These walk the AST to find definition sites, references, and
//! read/write occurrences of an identifier. They are pure functions over
//! the parsed `SourceFile` and source text; the `Backend` request
//! handlers call them after resolving the identifier under the cursor.

#[cfg(test)]
use std::collections::HashMap;

use ry_core::{Expr, SourceFile, Span, Stmt};
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, Range, Url,
};

#[cfg(test)]
use tower_lsp::lsp_types::{TextEdit, WorkspaceEdit};

use crate::util::byte_offset_to_position;

/// Find every definition site of `name` in `file`, returning each as an
/// LSP `Location` inside `uri`.
pub(super) fn find_definition_locations(file: &SourceFile, name: &str, uri: &Url) -> Vec<Location> {
    let mut spans: Vec<Span> = Vec::new();
    for stmt in &file.stmts {
        find_def_spans_in_stmt(stmt, name, &mut spans);
    }
    spans
        .into_iter()
        .map(|sp| span_to_location(sp, name, uri))
        .collect()
}

/// Convert a definition-site `Span` into an LSP `Location`. The range
/// highlights the identifier itself (col .. col + name.len()).
fn span_to_location(span: Span, name: &str, uri: &Url) -> Location {
    let start = Position {
        line: span.line as u32,
        character: span.col as u32,
    };
    let end = Position {
        line: span.line as u32,
        character: span.col as u32 + name.len() as u32,
    };
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
            span,
            ..
        } => {
            if loop_var == name {
                out.push(*span);
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
            span,
        } => {
            if include_declaration && loop_var == name {
                out.push(*span);
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

/// Walk the AST of `file` collecting every occurrence of `name` in the
/// current file and classify each as a READ or WRITE highlight.
pub(super) fn collect_document_highlights(
    file: &SourceFile,
    name: &str,
    text: &str,
) -> Vec<DocumentHighlight> {
    let mut entries: Vec<(Span, DocumentHighlightKind)> = Vec::new();
    for stmt in &file.stmts {
        collect_highlight_entries_from_stmt(stmt, name, &mut entries);
    }
    entries
        .into_iter()
        .map(|(span, kind)| DocumentHighlight {
            range: span_to_visible_range(span, text),
            kind: Some(kind),
        })
        .collect()
}

fn collect_highlight_entries_from_stmt(
    stmt: &Stmt,
    name: &str,
    out: &mut Vec<(Span, DocumentHighlightKind)>,
) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            if let Expr::Ident { name: n, span } = target {
                if n == name {
                    out.push((*span, DocumentHighlightKind::WRITE));
                }
            }
            collect_highlight_entries_from_expr(value, name, out);
        }
        Stmt::FunctionDef {
            name: fn_name,
            body,
            span,
            ..
        } => {
            if let Some(n) = fn_name {
                if n == name {
                    out.push((*span, DocumentHighlightKind::WRITE));
                }
            }
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            collect_highlight_entries_from_expr(cond, name, out);
            for s in then {
                collect_highlight_entries_from_stmt(s, name, out);
            }
            if let Some(else_block) = else_ {
                for s in else_block {
                    collect_highlight_entries_from_stmt(s, name, out);
                }
            }
        }
        Stmt::For {
            name: loop_var,
            iter,
            body,
            span,
        } => {
            if loop_var == name {
                out.push((*span, DocumentHighlightKind::WRITE));
            }
            collect_highlight_entries_from_expr(iter, name, out);
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_highlight_entries_from_expr(cond, name, out);
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                collect_highlight_entries_from_expr(v, name, out);
            }
        }
        Stmt::Expr(e) => collect_highlight_entries_from_expr(e, name, out),
    }
}

fn collect_highlight_entries_from_expr(
    expr: &Expr,
    name: &str,
    out: &mut Vec<(Span, DocumentHighlightKind)>,
) {
    match expr {
        Expr::Ident { name: n, span } => {
            if n == name {
                out.push((*span, DocumentHighlightKind::READ));
            }
        }
        Expr::Call { func, args, .. } => {
            collect_highlight_entries_from_expr(func, name, out);
            for arg in args {
                collect_highlight_entries_from_expr(&arg.value, name, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_highlight_entries_from_expr(lhs, name, out);
            collect_highlight_entries_from_expr(rhs, name, out);
        }
        Expr::UnaryOp { expr, .. } => collect_highlight_entries_from_expr(expr, name, out),
        Expr::Index { base, args, .. } => {
            collect_highlight_entries_from_expr(base, name, out);
            for arg in args {
                collect_highlight_entries_from_expr(&arg.value, name, out);
            }
        }
        Expr::Function { body, .. } => {
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_highlight_entries_from_expr(cond, name, out);
            collect_highlight_entries_from_expr(then, name, out);
            if let Some(e) = else_ {
                collect_highlight_entries_from_expr(e, name, out);
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

/// Convert a `Span`'s byte offsets to an LSP `Range` against `text`.
/// Zero-width spans are widened by one character.
fn span_to_visible_range(span: Span, text: &str) -> Range {
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
    Range { start, end }
}

/// Build a `WorkspaceEdit` renaming `old_name` to `new_name` across the
/// given slice of `(path, parsed_file, source_text)` tuples. Unit-test
/// mirror of the `rename` LSP method.
#[cfg(test)]
pub(super) fn build_rename_edits(
    docs: &[(&str, &SourceFile, &str)],
    old_name: &str,
    new_name: &str,
) -> WorkspaceEdit {
    let mut edits: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for (doc_path, file, doc_text) in docs {
        let doc_uri = crate::backend::path_to_uri(doc_path);
        let locations = find_references_in_file(file, old_name, &doc_uri, doc_text, true);
        for loc in locations {
            edits.entry(doc_uri.clone()).or_default().push(TextEdit {
                range: loc.range,
                new_text: new_name.to_string(),
            });
        }
    }
    WorkspaceEdit {
        changes: Some(edits),
        ..Default::default()
    }
}
