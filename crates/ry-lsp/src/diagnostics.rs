//! Diagnostic-conversion and code-action helpers.
//!
//! These translate ry's own `Diagnostic` type into LSP `Diagnostic`s
//! (with precise byte-offset-derived ranges) and build the `CodeAction`s
//! offered by the `code_action` handler (suppress-on-line,
//! suppress-in-file). They are pure functions over public types, so they
//! live outside the `Backend` impl.

use std::collections::HashMap;

use ry_checker::{Diagnostic as RyDiagnostic, Severity};
use ry_core::SourceFile;
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

/// Build a line suppression, merging existing rule lists and preserving prose.
/// Refuse edits that could insert comment text inside a multiline token. Returns `None` when the
/// checker would already suppress the diagnostic (no redundant no-op):
/// either a trailing directive on its line or a standalone directive on
/// the comment-only lines directly above it.
pub(super) fn make_ignore_action(
    uri: &Url,
    diag: &LspDiagnostic,
    file: &SourceFile,
) -> Option<CodeAction> {
    let text = &file.source;
    let line = diag.range.start.line as usize;
    let line_text = text.lines().nth(line)?;
    let code = diag_code_from_lsp(diag);

    let suppressions = ry_checker::parse_suppressions_from_comments(&file.comments, text);
    let already_ignored = suppressions.iter().any(|suppression| {
        suppression.line == line
            && (suppression.rules.is_empty() || suppression.rules.iter().any(|rule| rule == &code))
    });
    if already_ignored {
        return None;
    }

    if !file.parse_errors.is_empty() {
        return None;
    }
    let comment = file.comments.iter().find(|comment| comment.line == line);
    let trailing = comment.map_or_else(Vec::new, |comment| {
        ry_checker::parse_suppressions_from_comments(std::slice::from_ref(comment), text)
    });
    let directive = if code.is_empty() {
        if comment.is_some() {
            "# ry: ignore[]"
        } else {
            "# ry: ignore"
        }
        .to_string()
    } else {
        let mut codes = trailing
            .iter()
            .flat_map(|s| s.rules.iter().cloned())
            .collect::<Vec<_>>();
        codes.push(code.clone());
        codes.sort();
        codes.dedup();
        format!("# ry: ignore[{}]", codes.join(", "))
    };
    let new_line = match comment {
        Some(comment)
            if !trailing.is_empty()
                && !comment
                    .body
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("noqa:") =>
        {
            // Replace bracketed directives. Colon-form noqa has no delimiter
            // separating codes from prose, so preserve that comment below.
            let suffix = comment
                .body
                .find(']')
                .map_or("", |end| &comment.body[end + 1..]);
            format!("{}{}{}", &line_text[..comment.col], directive, suffix)
        }
        Some(comment) => format!(
            "{}{}  {}",
            &line_text[..comment.col],
            directive,
            &line_text[comment.col..]
        ),
        None => format!("{line_text}  {directive}"),
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
    let line_end = line_start_byte + line_text.len();
    let end = byte_offset_to_position(text, line_end);

    // Parse the proposed edit, not the cached source: the new marker must
    // become a comment rather than text inside a multiline string or name.
    let mut edited = text.to_string();
    edited.replace_range(line_start_byte..line_end, &new_line);
    let mut parser = ry_core::RParser::new().ok()?;
    let edited_file = parser.parse(&file.path, &edited).ok()?;
    let marker_col = comment.map_or(line_text.len() + 2, |comment| comment.col);
    if !edited_file
        .comments
        .iter()
        .any(|comment| comment.line == line && comment.col == marker_col)
    {
        return None;
    }

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

/// Build a file suppression at the top of the document, after any shebang. Returns
/// `None` when the file already carries a file-level suppression.
pub(super) fn make_ignore_file_action(uri: &Url, file: &SourceFile) -> Option<CodeAction> {
    if ry_checker::has_file_suppression_from_comments(&file.comments) {
        return None;
    }

    let shebang = file.source.starts_with("#!");
    let offset = if shebang {
        file.source
            .find('\n')
            .map_or(file.source.len(), |end| end + 1)
    } else {
        0
    };
    let position = byte_offset_to_position(&file.source, offset);
    let prefix = if shebang && !file.source.contains('\n') {
        "\n"
    } else {
        ""
    };
    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: position,
                end: position,
            },
            new_text: format!("{prefix}# ry: ignore-file\n"),
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
