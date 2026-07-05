//! AST-based identifier-at-offset lookup.
//!
//! `find_ident_at_offset` walks the whole AST to resolve the innermost
//! `Expr::Ident` whose span contains a byte offset, used by hover,
//! go-to-definition, references, rename, prepare-rename, and document
//! highlights. Keywords and numeric literals are filtered out.

use ry_core::{Expr, SourceFile, Span, Stmt};

/// Find the smallest `Expr::Ident` whose `Span` contains `byte_offset`,
/// returning `(name, span)`. "Smallest" = the innermost identifier whose
/// span contains the offset; ties are broken by span length (shortest
/// wins).
pub(super) fn find_ident_at_offset(
    file: &SourceFile,
    byte_offset: usize,
) -> Option<(String, Span)> {
    let mut best: Option<(String, Span)> = None;
    for stmt in &file.stmts {
        find_ident_in_stmt(stmt, byte_offset, &mut best);
    }
    best
}

fn span_contains(span: Span, offset: usize) -> bool {
    offset >= span.start && offset < span.end
}

fn consider(name: &str, span: Span, offset: usize, best: &mut Option<(String, Span)>) {
    if !span_contains(span, offset) {
        return;
    }
    if is_numeric_or_keyword(name) {
        return;
    }
    let is_better = best
        .as_ref()
        .map(|(_, b)| span.end - span.start < b.end - b.start)
        .unwrap_or(true);
    if is_better {
        *best = Some((name.to_string(), span));
    }
}

fn find_ident_in_stmt(s: &Stmt, offset: usize, best: &mut Option<(String, Span)>) {
    match s {
        Stmt::Assign { target, value, .. } => {
            find_ident_in_expr(target, offset, best);
            find_ident_in_expr(value, offset, best);
        }
        Stmt::Expr(e) => find_ident_in_expr(e, offset, best),
        Stmt::If {
            cond, then, else_, ..
        } => {
            find_ident_in_expr(cond, offset, best);
            for s in then {
                find_ident_in_stmt(s, offset, best);
            }
            if let Some(e) = else_ {
                for s in e {
                    find_ident_in_stmt(s, offset, best);
                }
            }
        }
        Stmt::For {
            iter,
            body,
            name,
            span,
        } => {
            find_ident_in_expr(iter, offset, best);
            // The loop variable binding is a bare name (no inner span);
            // use the statement span as a coarse fallback.
            consider(name, *span, offset, best);
            for s in body {
                find_ident_in_stmt(s, offset, best);
            }
        }
        Stmt::While { cond, body, .. } => {
            find_ident_in_expr(cond, offset, best);
            for s in body {
                find_ident_in_stmt(s, offset, best);
            }
        }
        Stmt::FunctionDef {
            name,
            params,
            body,
            span,
        } => {
            if let Some(n) = name {
                consider(n, *span, offset, best);
            }
            for p in params {
                if let Some(d) = &p.default {
                    find_ident_in_expr(d, offset, best);
                }
            }
            for s in body {
                find_ident_in_stmt(s, offset, best);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                find_ident_in_expr(v, offset, best);
            }
        }
    }
}

fn find_ident_in_expr(e: &Expr, offset: usize, best: &mut Option<(String, Span)>) {
    match e {
        Expr::Ident { name, span } => consider(name, *span, offset, best),
        Expr::BinOp { lhs, rhs, .. } => {
            find_ident_in_expr(lhs, offset, best);
            find_ident_in_expr(rhs, offset, best);
        }
        Expr::UnaryOp { expr, .. } => find_ident_in_expr(expr, offset, best),
        Expr::Call { func, args, .. } => {
            find_ident_in_expr(func, offset, best);
            for a in args {
                find_ident_in_expr(&a.value, offset, best);
            }
        }
        Expr::Index { base, args, .. } => {
            find_ident_in_expr(base, offset, best);
            for a in args {
                find_ident_in_expr(&a.value, offset, best);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            find_ident_in_expr(cond, offset, best);
            find_ident_in_expr(then, offset, best);
            if let Some(e) = else_ {
                find_ident_in_expr(e, offset, best);
            }
        }
        Expr::Function { params, body, .. } => {
            for p in params {
                if let Some(d) = &p.default {
                    find_ident_in_expr(d, offset, best);
                }
            }
            for s in body {
                find_ident_in_stmt(s, offset, best);
            }
        }
        _ => {}
    }
}

/// True if `name` is a pure number or an R reserved word -- rename/hover/
/// go-to-def ignore keywords and numeric literals.
pub(super) fn is_numeric_or_keyword(name: &str) -> bool {
    if name.parse::<f64>().is_ok() {
        return true;
    }
    matches!(
        name,
        "if" | "else"
            | "for"
            | "while"
            | "function"
            | "return"
            | "break"
            | "next"
            | "repeat"
            | "in"
            | "TRUE"
            | "FALSE"
            | "NULL"
            | "NA"
            | "NA_integer_"
            | "NA_real_"
            | "NA_complex_"
            | "NA_character_"
            | "Inf"
            | "NaN"
            | "T"
            | "F"
            | "library"
            | "require"
    )
}
