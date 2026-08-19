//! Position / range conversion helpers for the LSP layer.
//!
//! The LSP spec defines `Position.character` as a UTF-16 code-unit
//! offset. These helpers convert between byte offsets (what tree-sitter
//! and ry's `Span` use) and LSP `Position`s, counting UTF-16 code units
//! so non-ASCII (including astral-plane) characters resolve correctly.
//!
//! Extracted from `lib.rs` because they
//! are pure functions with no dependency on the `Backend`/`State` and
//! are reused across every LSP handler.

use tower_lsp::lsp_types::Position;

/// Map a byte offset into the source text to an LSP `Position`
/// (0-indexed line, 0-indexed character column).
///
/// The LSP spec defines `Position.character` as a UTF-16 code-unit
/// offset. This helper counts UTF-16 code units (each BMP character is
/// 1 unit; astral-plane characters -- emoji, rare CJK -- are 2). For
/// pure ASCII source the count equals the byte count.
pub(crate) fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (b, ch) in text.char_indices() {
        if b >= byte_offset {
            break;
        }
        match ch {
            '\r' if text.as_bytes().get(b + 1) == Some(&b'\n') => {
                // The `\r` of a CRLF line terminator is not a column
                // character; the following `\n` handles the line break.
            }
            '\n' => {
                line += 1;
                col = 0;
            }
            _ => col += utf16_len(ch) as u32,
        }
    }
    Position {
        line,
        character: col,
    }
}

/// Number of UTF-16 code units a Unicode scalar value encodes to: 1 for
/// the Basic Multilingual Plane, 2 for astral-plane characters (which
/// become a surrogate pair).
pub(crate) fn utf16_len(ch: char) -> usize {
    if (ch as u32) >= 0x10000 { 2 } else { 1 }
}

/// Map an LSP `Position` (line, UTF-16 character column) to a byte
/// offset into the source text. The inverse of `byte_offset_to_position`.
pub(crate) fn position_to_byte_offset(text: &str, line: u32, utf16_col: u32) -> Option<usize> {
    let mut cur_line = 0u32;
    let mut cur_col = 0u32;
    for (b, ch) in text.char_indices() {
        if cur_line == line {
            if cur_col == utf16_col {
                return Some(b);
            }
            // No byte boundary exists inside an astral scalar's UTF-16
            // surrogate pair. Reject that position instead of snapping it
            // forward to the next scalar.
            if cur_col > utf16_col {
                return None;
            }
        }
        match ch {
            '\r' if text.as_bytes().get(b + 1) == Some(&b'\n') => {
                // CRLF: the `\r` is part of the line terminator, not a
                // column character; only the `\n` resets the column.
            }
            '\n' => {
                cur_line += 1;
                cur_col = 0;
            }
            _ => cur_col += utf16_len(ch) as u32,
        }
    }
    if cur_line == line && cur_col == utf16_col {
        Some(text.len())
    } else {
        None
    }
}

/// Map an LSP `Position` to a byte offset. Wrapper over the line/col
/// variant for callers that hold a `Position`. Returns `None` when the
/// position does not fall inside the text (line past the end of the
/// file, or a column past the end of its line), so callers can report
/// "no result" instead of silently resolving against the last byte.
pub(crate) fn position_to_byte_offset_pos(text: &str, position: Position) -> Option<usize> {
    position_to_byte_offset(text, position.line, position.character)
}

/// Convert a UTF-16 code-unit column (LSP) to a byte offset within a
/// single line. Returns `None` when the column lands past the end of
/// the line or inside a surrogate pair.
pub(crate) fn utf16_col_to_byte(line: &str, utf16_col: u32) -> Option<usize> {
    let mut col = 0u32;
    for (byte, ch) in line.char_indices() {
        if col == utf16_col {
            return Some(byte);
        }
        let next_col = col + utf16_len(ch) as u32;
        if utf16_col < next_col {
            return None;
        }
        col = next_col;
    }
    (col == utf16_col).then_some(line.len())
}
