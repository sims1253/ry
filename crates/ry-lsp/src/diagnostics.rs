//! Diagnostic-conversion and code-action helpers (PLAN Phase E3).
//!
//! These translate ry's own `Diagnostic` type into LSP `Diagnostic`s
//! (with precise byte-offset-derived ranges) and build the `CodeAction`s
//! offered by the `code_action` handler (suppress-on-line,
//! suppress-in-file). They are pure functions over public types, so they
//! live outside the `Backend` impl.

use std::collections::HashMap;

use ry_checker::{Diagnostic as RyDiagnostic, Severity};
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Diagnostic as LspDiagnostic, DiagnosticSeverity, NumberOrString,
    Position, Range, TextEdit, Url, WorkspaceEdit,
};

use crate::util::byte_offset_to_position;

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
/// against the source text. The production path
/// (`publish_diagnostics`); editors squiggle exactly the offending
/// token. Zero-width spans are extended by one character so the squiggle
/// is still visible.
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

/// Build a `CodeAction` that appends a `# ry: ignore[CODE]` suppression
/// comment to the end of the diagnostic's line. Returns `None` when the
/// line already carries an ignore comment (no redundant no-op).
pub(super) fn make_ignore_action(
    uri: &Url,
    diag: &LspDiagnostic,
    text: &str,
) -> Option<CodeAction> {
    let line = diag.range.start.line as usize;
    let line_text = text.lines().nth(line)?;

    if line_text.contains("ry: ignore") {
        return None;
    }

    let code = diag_code_from_lsp(diag);
    let new_line = if code.is_empty() {
        format!("{}  # ry: ignore", line_text)
    } else {
        format!("{}  # ry: ignore[{}]", line_text, code)
    };

    let start = Position {
        line: diag.range.start.line,
        character: 0,
    };
    let end = Position {
        line: diag.range.start.line,
        character: line_text.len() as u32,
    };

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
pub(super) fn make_ignore_file_action(uri: &Url, text: &str) -> Option<CodeAction> {
    if text.contains("ry: ignore-file") {
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
