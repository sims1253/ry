//! Conversion of ry diagnostics to LSP diagnostics, and the suppression
//! code actions.

use std::collections::HashMap;

use ry_checker::{Diagnostic as RyDiagnostic, Severity};
use ry_core::SourceFile;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Diagnostic as LspDiagnostic, DiagnosticRelatedInformation,
    DiagnosticSeverity, Location, NumberOrString, Position, Range, TextEdit, Url, WorkspaceEdit,
};

use crate::positions::{byte_offset_to_position, line_start};

/// Identify the document and analysis snapshot that produced a diagnostic.
/// Clients advertising diagnostic data support preserve this in code actions.
pub(super) fn diagnostic_origin(path: &str, version: i32, generation: u64) -> serde_json::Value {
    serde_json::json!({"ry": {"path": path, "version": version, "generation": generation}})
}

/// Convert a ry diagnostic, ranging over its span's bytes in `text`. A
/// zero-width span widens by one character so the squiggle stays visible.
///
/// `related_uri` is the document's own URI when the client supports related
/// information and the display path names exactly one document. Related
/// items in this same file whose spans fit `text` then become
/// `relatedInformation`; others are dropped, never relocated.
pub(super) fn diagnostic_to_lsp_with_source(
    d: &RyDiagnostic,
    text: &str,
    related_uri: Option<&Url>,
) -> LspDiagnostic {
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
        related_information: related_uri.and_then(|uri| {
            let related: Vec<_> = d
                .related
                .iter()
                .filter(|related| related.path == d.path)
                .filter_map(|related| {
                    let range = related.range_in(text)?;
                    Some(DiagnosticRelatedInformation {
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: byte_offset_to_position(text, range.start),
                                end: byte_offset_to_position(text, range.end),
                            },
                        },
                        message: related.message.clone(),
                    })
                })
                .collect();
            (!related.is_empty()).then_some(related)
        }),
        ..Default::default()
    }
}

/// The diagnostic's code, or an empty string without one, in which case
/// the ignore comment has no `[CODE]` suffix.
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
    let already_ignored = suppressions
        .iter()
        .any(|suppression| suppression.line == line && suppression.suppresses(&code));
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
            .flat_map(|s| s.valid_rules().unwrap_or_default().iter().cloned())
            .collect::<Vec<_>>();
        codes.push(code.clone());
        codes.sort();
        codes.dedup();
        format!("# ry: ignore[{}]", codes.join(", "))
    };
    let new_line = match comment {
        Some(comment)
            if !trailing.is_empty() && {
                let body = comment.body.trim_start().to_ascii_lowercase();
                ["ry: ignore", "ry:ignore", "noqa"].iter().any(|marker| {
                    body.strip_prefix(marker).is_some_and(|rest| {
                        rest.trim_start().starts_with('[') && rest.contains(']')
                    })
                })
            } =>
        {
            // Only bracketed directives delimit their codes from trailing prose.
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
    // `Position.character` counts UTF-16 units, not bytes.
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
    // A malformed suffix can survive a textual replacement and leave the
    // proposed ignore invalid. Ask the checker's parser whether this edit
    // actually suppresses the requested code before offering the action.
    if !ry_checker::parse_suppressions_from_comments(&edited_file.comments, &edited)
        .iter()
        .any(|suppression| {
            suppression.line == line
                && if code.is_empty() {
                    suppression.kind == ry_checker::SuppressionKind::All
                } else {
                    suppression.suppresses(&code)
                }
        })
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
