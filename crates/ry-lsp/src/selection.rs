//! Selection-range and identifier-range helpers.

use ry_core::{SourceFile, Span};
use tower_lsp::lsp_types::{Position, Range, SelectionRange};

use crate::folding::span_of_stmt;
use crate::util::{byte_offset_to_position, position_to_byte_offset_pos, span_to_range};

/// Find the identifier (variable name) at a given line and column in
/// the source text, returning BOTH the identifier string AND its LSP
/// `Range` (line + character offsets of the identifier span). Returns
/// `None` if the position is not on an identifier-like character
/// sequence.
///
/// The search expands left and right from the cursor to find the
/// boundaries of the word. Filtering rules: pure numbers and R keywords
/// are rejected (they are not renameable bindings).
///
/// Used by `prepare_rename` to validate that the cursor sits on a
/// renameable identifier before the editor shows the rename UI, and
/// to hand the editor the exact span to highlight as the rename
/// target.
pub(super) fn find_identifier_range_at_position(
    text: &str,
    line: usize,
    col: usize,
) -> Option<(String, Range)> {
    let line_str = text.lines().nth(line)?;
    let bytes = line_str.as_bytes();
    if bytes.is_empty() || col >= bytes.len() {
        return None;
    }
    // The character at the cursor must be identifier-like.
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.';
    if !is_ident_char(bytes[col]) {
        // Check if the cursor is just after an identifier (common when
        // the user places the cursor right at the end of a word).
        if col > 0 && is_ident_char(bytes[col - 1]) {
            // Expand from col-1 instead.
        } else {
            return None;
        }
    }
    // Expand left to find the start of the identifier.
    let mut start = col;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }
    // Expand right to find the end.
    let mut end = col;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }
    if start >= end {
        return None;
    }
    let ident = std::str::from_utf8(&bytes[start..end]).ok()?;
    // Filter out pure-number identifiers (123) and reserved words.
    if ident.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Filter out R keywords that are not variable bindings.
    if matches!(
        ident,
        "if" | "else"
            | "for"
            | "while"
            | "repeat"
            | "function"
            | "return"
            | "break"
            | "next"
            | "TRUE"
            | "FALSE"
            | "NULL"
            | "NA"
            | "Inf"
            | "NaN"
            | "in"
    ) {
        return None;
    }
    let range = Range {
        start: Position {
            line: line as u32,
            character: start as u32,
        },
        end: Position {
            line: line as u32,
            character: end as u32,
        },
    };
    Some((ident.to_string(), range))
}
/// Find the smallest enclosing statement whose `Span` contains the
/// cursor position, returning its range as an LSP `Range`. Returns
/// `None` when the cursor is not inside any statement (e.g. on a
/// blank line past the end of the file).
///
/// The byte offset of the cursor is computed via
/// `position_to_byte_offset`; we then walk the file's top-level
/// statements and return the first whose span contains that offset.
/// Nested statements (function bodies, control-flow blocks) are not
/// searched: for the expand-selection use case, expanding from the
/// identifier to the top-level statement is the most useful single
/// hop, and the file-level range provides the "expand all the way"
/// step.
fn find_enclosing_stmt_range(position: Position, file: &SourceFile, text: &str) -> Option<Range> {
    let byte_offset = position_to_byte_offset_pos(text, position);
    let mut best: Option<Span> = None;
    for stmt in &file.stmts {
        if let Some(span) = span_of_stmt(stmt) {
            if byte_offset >= span.start && byte_offset < span.end {
                // Prefer the smallest (innermost) enclosing statement
                // so multi-statement lines and nested constructs
                // expand to the tightest meaningful range first.
                match best {
                    None => best = Some(span),
                    Some(prev) if span.end - span.start < prev.end - prev.start => {
                        best = Some(span);
                    }
                    _ => {}
                }
            }
        }
    }
    best.and_then(|span| span_to_range(text, span))
}

/// Compute an LSP `Range` covering the entire source text (from
/// `(0, 0)` to the position of the last byte). Used as the widest
/// level of the selection-range chain.
fn file_range(text: &str) -> Range {
    let end = byte_offset_to_position(text, text.len());
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end,
    }
}

/// Build a `SelectionRange` chain for a single cursor position. The
/// chain widens from the identifier under the cursor (narrowest) to
/// the enclosing statement (middle) to the whole file (widest),
/// matching how VS Code's "Expand Selection" works.
///
/// When the cursor is not on an identifier (e.g. on whitespace or an
/// operator), the narrowest range falls back to a zero-width span at
/// the cursor so the editor still has something to anchor the
/// selection. Levels that would be identical to their child (e.g. a
/// single-statement file where the statement span equals the file
/// span) are skipped so the chain never contains duplicate ranges.
pub(super) fn build_selection_range(
    position: Position,
    file: &SourceFile,
    text: &str,
) -> SelectionRange {
    // 1. The identifier at the cursor (narrowest). Fall back to a
    //    zero-width span at the cursor when the position is not on an
    //    identifier-like character.
    let ident_range = find_identifier_range_at_position(
        text,
        position.line as usize,
        position.character as usize,
    )
    .map(|(_, r)| r)
    .unwrap_or(Range {
        start: position,
        end: position,
    });

    // 2. The enclosing statement (middle).
    let stmt_range = find_enclosing_stmt_range(position, file, text);

    // 3. The whole file (widest).
    let file_range = file_range(text);

    // Build the chain from widest to narrowest so each level's
    // `parent` points to the next wider range. Duplicate levels are
    // skipped so the editor never offers a no-op expand step.
    //
    // Per the LSP spec, the outermost `SelectionRange` is the
    // narrowest; each `parent` widens. We therefore start from the
    // widest range (file) as the deepest parent and wrap outward.
    let mut chain: Vec<Range> = vec![file_range];
    if let Some(stmt) = stmt_range {
        if stmt != file_range && stmt != ident_range {
            chain.push(stmt);
        }
    }
    if ident_range != *chain.last().unwrap() {
        chain.push(ident_range);
    }

    // Fold the chain into nested `SelectionRange`s. The chain is
    // ordered widest-first; we build from the widest (deepest parent)
    // and wrap each narrower level around it so the outermost
    // `SelectionRange.range` is the narrowest (identifier).
    let mut sel = SelectionRange {
        range: chain[0],
        parent: None,
    };
    for r in chain.into_iter().skip(1) {
        sel = SelectionRange {
            range: r,
            parent: Some(Box::new(sel)),
        };
    }
    sel
}
