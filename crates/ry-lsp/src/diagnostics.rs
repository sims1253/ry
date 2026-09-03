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

/// Build a `CodeAction` that appends a `# ry: ignore[CODE]` suppression
/// comment to the end of the diagnostic's line. Returns `None` when the
/// checker would already suppress the diagnostic (no redundant no-op):
/// either a trailing directive on its line or a standalone directive on
/// the comment-only lines directly above it.
pub(super) fn make_ignore_action(
    uri: &Url,
    diag: &LspDiagnostic,
    text: &str,
) -> Option<CodeAction> {
    let line = diag.range.start.line as usize;
    let lines: Vec<&str> = text.lines().collect();
    let line_text = *lines.get(line)?;
    let code = diag_code_from_lsp(diag);

    // Avoid a redundant action when the source already carries a
    // suppression directive the CHECKER would honor for THIS
    // diagnostic. The check reuses ry-checker's suppression parser
    // (the same one publish_diagnostics filters through) so the
    // quick-fix's notion of "already ignored" cannot drift from what
    // is actually suppressed: the directive must START the comment
    // body (after `#` and whitespace), not merely appear as a
    // substring (so prose like "# See docs for ry: ignore" does not
    // block the action), and `# ry: ignore-file` — a file-level,
    // not line-level, directive — does not block it either. A rule
    // list must also cover the diagnostic: `# ry: ignore[RY010]`
    // suppresses only RY010, so the quick-fix stays available for
    // other codes (appending a second directive is a meaningful
    // edit, not a no-op), while a bare `# ry: ignore` — an empty
    // rule list, "all rules" — blocks any code.
    //
    // The parser is fed a bounded window, not the diagnostic's single
    // line: the checker assigns a STANDALONE directive to the next
    // non-comment, non-blank line, so a directive on the comment-only
    // lines above the diagnostic already suppresses it. Only the
    // contiguous run of blank / comment-only lines ending just above
    // the diagnostic's line (plus the line itself) can matter — any
    // directive higher up is absorbed by the code line that ends that
    // run, exactly as the checker's own line-based resolution skips
    // blanks and comments downward. `suppression.line == target`
    // keeps the match on the diagnostic's line: a trailing directive
    // resolves to the line it sits on, a standalone one to the
    // window's last line, and a standalone directive that IS the
    // diagnostic's whole line finds no next line and suppresses
    // nothing.
    let window_start = suppression_window_start(&lines, line);
    let window = lines[window_start..=line].join("\n");
    let target = line - window_start;
    let already_ignored = RParser::new()
        .ok()
        .and_then(|mut parser| parser.parse("<code-action>", &window).ok())
        .map(|file| {
            ry_checker::parse_suppressions_from_comments(&file.comments, &window)
                .iter()
                .any(|suppression| {
                    suppression.line == target
                        && (suppression.rules.is_empty()
                            || suppression.rules.iter().any(|rule| rule == &code))
                })
        })
        .unwrap_or(false);
    if already_ignored {
        return None;
    }

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

/// The first line of the bounded suppression window for a diagnostic on
/// `line`: the top of the contiguous run of blank / comment-only lines
/// ending just above it, or `line` itself when the line above carries
/// code. Line classification mirrors the checker's `next_code_line`
/// (blank, or first non-whitespace character is `#`) so a standalone
/// directive anywhere in the run resolves onto `line` under the same
/// rules the checker applies to the full document, while a directive
/// above the run is absorbed by the code line that ends it.
fn suppression_window_start(lines: &[&str], line: usize) -> usize {
    let mut start = line;
    while start > 0 {
        let trimmed = lines[start - 1].trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            start -= 1;
        } else {
            break;
        }
    }
    start
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
