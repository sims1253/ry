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

use ry_core::Span;
use tower_lsp::lsp_types::{Position, Range};

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
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += utf16_len(ch) as u32;
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
    if (ch as u32) >= 0x10000 {
        2
    } else {
        1
    }
}

/// Map an LSP `Position` (line, UTF-16 character column) to a byte
/// offset into the source text. The inverse of `byte_offset_to_position`.
pub(crate) fn position_to_byte_offset(text: &str, line: u32, utf16_col: u32) -> Option<usize> {
    let mut cur_line = 0u32;
    let mut cur_col = 0u32;
    for (b, ch) in text.char_indices() {
        if cur_line == line && cur_col >= utf16_col {
            return Some(b);
        }
        if ch == '\n' {
            cur_line += 1;
            cur_col = 0;
        } else {
            cur_col += utf16_len(ch) as u32;
        }
    }
    if cur_line == line && cur_col >= utf16_col {
        Some(text.len())
    } else {
        None
    }
}

/// Map an LSP `Position` to a byte offset. Wrapper over the line/col
/// variant for callers that hold a `Position`.
pub(crate) fn position_to_byte_offset_pos(text: &str, position: Position) -> usize {
    position_to_byte_offset(text, position.line, position.character).unwrap_or(text.len())
}

/// Convert a ry `Span` (byte offsets) to an LSP `Range` (UTF-16
/// positions). Both endpoints go through `byte_offset_to_position` so
/// the character column is a UTF-16 code-unit count and start/end are
/// computed consistently.
pub(crate) fn span_to_range(text: &str, span: Span) -> Option<Range> {
    let start = byte_offset_to_position(text, span.start);
    let end = byte_offset_to_position(text, span.end);
    Some(Range { start, end })
}
