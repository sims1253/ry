//! Diagnostic-conversion and code-action helpers.
//!
//! These translate ry's own `Diagnostic` type into LSP `Diagnostic`s
//! (with precise byte-offset-derived ranges) and build the `CodeAction`s
//! offered by the `code_action` handler (suppress-on-line,
//! suppress-in-file). They are pure functions over public types, so they
//! live outside the `Backend` impl.

use std::collections::HashMap;

use ry_checker::{Diagnostic as RyDiagnostic, Severity};
use ry_core::RParser;
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
    let data = d.fix.as_ref().map(|fix| {
        let fix_start = Position {
            line: fix.span.line as u32,
            character: fix.span.col as u32,
        };
        let fix_end = Position {
            line: fix.span.line as u32,
            character: (fix.span.col + fix.span.end.saturating_sub(fix.span.start)) as u32,
        };
        serde_json::json!({
            "fix": {
                "range": {"start": fix_start, "end": fix_end},
                "replacement": fix.replacement,
            }
        })
    });
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
        data,
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
    let data = d.fix.as_ref().map(|fix| {
        let start = byte_offset_to_position(text, fix.span.start);
        let end = byte_offset_to_position(text, fix.span.end);
        serde_json::json!({
            "fix": {
                "range": {"start": start, "end": end},
                "replacement": fix.replacement,
            }
        })
    });
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
        data,
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

    // Avoid a redundant action when the line already carries a
    // suppression directive. Check that the marker STARTS the comment
    // body (after `#` and whitespace), not merely appears as a substring
    // (so prose like "# See docs for ry: ignore" does not block the action).
    let already_ignored = RParser::new()
        .ok()
        .and_then(|mut parser| parser.parse("<code-action>", line_text).ok())
        .into_iter()
        .flat_map(|file| file.comments)
        .map(|comment| comment.body.trim_start().to_lowercase())
        .any(|body| {
            body.starts_with("ry: ignore")
                || body.starts_with("ry:ignore")
                || body.starts_with("noqa")
        });
    if already_ignored {
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
    // `line_text.len()` is a BYTE length but `Position.character` is a
    // UTF-16 code-unit column, so convert the byte offset of the line's
    // end to a proper column (a non-ASCII line would otherwise produce
    // an out-of-range character).
    let line_start = line_start_byte_offset(text, line);
    let end = byte_offset_to_position(text, line_start + line_text.len());

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

/// Byte offset of the first character of the given 0-indexed line.
fn line_start_byte_offset(text: &str, line: usize) -> usize {
    let mut offset = 0usize;
    for (i, piece) in text.split_inclusive('\n').enumerate() {
        if i == line {
            break;
        }
        offset += piece.len();
    }
    offset
}
