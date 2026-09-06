//! Diagnostic-conversion and code-action helpers.
//!
//! These translate ry's own `Diagnostic` type into LSP `Diagnostic`s
//! (with precise byte-offset-derived ranges) and build the `CodeAction`s
//! offered by the `code_action` handler (suppress-on-line,
//! suppress-in-file). They are pure functions over public types, so they
//! live outside the `Backend` impl.

use std::collections::HashMap;
use std::ops::ControlFlow;

use ry_checker::{Diagnostic as RyDiagnostic, Severity, Suppression};
use ry_core::SourceFile;
use ry_core::ast::Expr;
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Diagnostic as LspDiagnostic, DiagnosticSeverity, NumberOrString,
    Position, Range, TextEdit, Url, WorkspaceEdit,
};

use crate::positions::{byte_offset_to_position, line_start};

/// Convert a `ry_checker::Diagnostic` to an LSP `Diagnostic` using the
/// span's pre-resolved `line` / `col` and a single-character range. Used
/// as a fallback (tests, missing source text); the production
/// diagnostics path uses [`diagnostic_to_lsp_with_source`].
pub(super) fn diagnostic_to_lsp(d: RyDiagnostic) -> LspDiagnostic {
    let start = Position {
        line: d.span.line as u32,
        character: d.span.col as u32,
    };
    let end = Position {
        line: d.span.line as u32,
        character: (d.span.col as u32) + 1,
    };
    let severity = match d.severity {
        Severity::Error => Some(DiagnosticSeverity::ERROR),
        Severity::Warning => Some(DiagnosticSeverity::WARNING),
        Severity::Info => Some(DiagnosticSeverity::INFORMATION),
    };
    LspDiagnostic {
        range: Range { start, end },
        severity,
        code: Some(NumberOrString::String(d.code.to_string())),
        source: Some("ry".to_string()),
        message: d.message,
        ..Default::default()
    }
}

/// Convert a `ry_checker::Diagnostic` to an LSP `Diagnostic` using a
/// precise multi-character range derived from the span's byte offsets
/// against the source text. This is the path `publish_diagnostics`
/// uses, so editors squiggle exactly the offending token. Zero-width
/// spans are extended by one character so the squiggle is still visible.
pub(super) fn diagnostic_to_lsp_with_source(d: &RyDiagnostic, text: &str) -> LspDiagnostic {
    let start = byte_offset_to_position(text, d.span.start);
    let end = byte_offset_to_position(text, d.span.end);
    let end = if start == end {
        Position {
            line: start.line,
            character: start.character + 1,
        }
    } else {
        end
    };
    let severity = match d.severity {
        Severity::Error => Some(DiagnosticSeverity::ERROR),
        Severity::Warning => Some(DiagnosticSeverity::WARNING),
        Severity::Info => Some(DiagnosticSeverity::INFORMATION),
    };
    LspDiagnostic {
        range: Range { start, end },
        severity,
        code: Some(NumberOrString::String(d.code.to_string())),
        source: Some("ry".to_string()),
        message: d.message.clone(),
        ..Default::default()
    }
}

/// Extract the diagnostic code string from an LSP `Diagnostic`. ry
/// always emits string codes (`RY040`, `RY001`, ...); the numeric
/// variant is handled defensively. Returns an empty string when the
/// diagnostic has no code, in which case the ignore comment omits the
/// `[CODE]` suffix.
pub(super) fn diag_code_from_lsp(d: &LspDiagnostic) -> String {
    match &d.code {
        Some(NumberOrString::String(s)) => s.clone(),
        Some(NumberOrString::Number(n)) => n.to_string(),
        None => String::new(),
    }
}

/// Build a `CodeAction` that suppresses the diagnostic with a
/// `# ry: ignore[CODE]` comment on its line. Returns `None` when the
/// checker would already suppress the diagnostic (no redundant no-op):
/// either a trailing directive on its line or a standalone directive on
/// the comment-only lines directly above it.
///
/// The edit merges into a comment that already sits on the line rather
/// than appending a second `#` marker after it — everything after the
/// first `#` is one comment and the checker only recognizes a directive
/// at the start of its body (#210). An existing directive's rule list is
/// extended in place; a prose comment keeps its text behind a fresh
/// directive. With no comment on the line one is appended — unless the
/// line ends inside an open multiline string, where an append would
/// change the string's value, so the action is withheld.
pub(super) fn make_ignore_action(
    uri: &Url,
    diag: &LspDiagnostic,
    file: &SourceFile,
    suppressions: &[Suppression],
) -> Option<CodeAction> {
    let text = &file.source;
    let line = diag.range.start.line as usize;
    let line_text = text.lines().nth(line)?;
    let code = diag_code_from_lsp(diag);

    let already_ignored = suppressions.iter().any(|suppression| {
        suppression.line == line
            && (suppression.rules.is_empty() || suppression.rules.iter().any(|rule| rule == &code))
    });
    if already_ignored {
        return None;
    }

    let new_line = match file.comments.iter().find(|c| c.line == line) {
        Some(comment) => {
            // A comment already trails the line. The checker only
            // recognizes a directive at the START of a comment body, so
            // merge into an existing directive or prepend a fresh one
            // ahead of the prose — never append a second `#` marker.
            // Trailing whitespace is trimmed because the whole-line
            // edit range ends before the line terminator, and a CRLF
            // `\r` swallowed by the comment node would otherwise be
            // duplicated.
            let body = comment.body.trim_end();
            let head = &line_text[..comment.col];
            match ry_checker::amend_ignore_comment_body(body, &code) {
                Some(amended) => format!("{head}#{amended}"),
                None if code.is_empty() => {
                    format!("{head}# ry: ignore {}", body.trim_start())
                }
                None => format!("{head}# ry: ignore[{code}] {}", body.trim_start()),
            }
        }
        None => {
            // No comment on the line: append one.
            if line_end_inside_string(file, line, line_text) {
                return None;
            }
            if code.is_empty() {
                format!("{line_text}  # ry: ignore")
            } else {
                format!("{line_text}  # ry: ignore[{code}]")
            }
        }
    };

    let start = Position {
        line: diag.range.start.line,
        character: 0,
    };
    // `line_text.len()` is a BYTE length but `Position.character` is a
    // UTF-16 code-unit column, so convert the byte offset of the line's
    // end to a proper column (a non-ASCII line would otherwise produce
    // an out-of-range character).
    let line_start_byte = line_start(text, line);
    let end = byte_offset_to_position(text, line_start_byte + line_text.len());

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range { start, end },
            new_text: new_line,
        }],
    );

    let title = if code.is_empty() {
        "Ignore this diagnostic on its line".to_string()
    } else {
        format!("Ignore {} on this line", code)
    };

    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        diagnostics: Some(vec![diag.clone()]),
        ..Default::default()
    })
}

/// Build a `CodeAction` that inserts `# ry: ignore-file` at the top of
/// the document, suppressing every ry diagnostic in the file. Returns
/// `None` when the file already carries a file-level suppression.
pub(super) fn make_ignore_file_action(uri: &Url, file: &SourceFile) -> Option<CodeAction> {
    if ry_checker::has_file_suppression_from_comments(&file.comments) {
        return None;
    }

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            new_text: "# ry: ignore-file\n".to_string(),
        }],
    );

    Some(CodeAction {
        title: "Ignore all diagnostics in this file".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Whether the end of `line` sits strictly inside a string literal —
/// a multiline string that opened on this line and continues below it.
/// A `#` appended there is string content, not a comment, so the edit
/// would silently change the string's value (#210); the caller
/// withholds the action instead.
fn line_end_inside_string(file: &SourceFile, line: usize, line_text: &str) -> bool {
    let line_end = line_start(&file.source, line) + line_text.len();
    let mut inside = false;
    let _ = walk_stmts(&file.stmts, Walk::ALL, |node, _| match node {
        AstNode::Expr(Expr::String(_, span)) if span.start < line_end && line_end < span.end => {
            inside = true;
            ControlFlow::Break(())
        }
        _ => ControlFlow::Continue(Descend::Into),
    });
    inside
}
