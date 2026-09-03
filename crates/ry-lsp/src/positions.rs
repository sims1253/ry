//! Line/column conversions for the LSP layer — one home for every
//! text-position walk in this crate.
//!
//! One line rule underlies them all: lines are delimited by `\n` only,
//! and the `\r` of a CRLF terminator is not a column character (a lone
//! `\r` is one). The helpers differ in the column unit they report:
//! UTF-16 code units for the LSP wire format ([`byte_offset_to_position`]
//! and [`position_to_byte_offset`], which share the [`Scan`] walk), and
//! bytes for tree-sitter `Point`s ([`byte_offset_to_point`]) and the
//! whole-line edit bounds ([`line_start`]).

use ry_core::Point;
use tower_lsp::lsp_types::Position;

/// The line/UTF-16-column state of a left-to-right scan: the shared
/// walk behind the two LSP wire-format conversions below.
#[derive(Default)]
struct Scan {
    line: u32,
    utf16_col: u32,
}

impl Scan {
    /// Consume the character at `byte`: a `\n` ends the line, the `\r`
    /// of a CRLF pair is not a column character, anything else advances
    /// the column by its UTF-16 encoded length.
    fn step(&mut self, ch: char, byte: usize, text: &str) {
        match ch {
            '\n' => {
                self.line += 1;
                self.utf16_col = 0;
            }
            '\r' if text.as_bytes().get(byte + 1) == Some(&b'\n') => {}
            _ => self.utf16_col += ch.len_utf16() as u32,
        }
    }
}

/// Map a byte offset into the source text to an LSP `Position`
/// (0-indexed line, 0-indexed character column).
///
/// The LSP spec defines `Position.character` as a UTF-16 code-unit
/// offset. This helper counts UTF-16 code units (each BMP character is
/// 1 unit; astral-plane characters -- emoji, rare CJK -- are 2). For
/// pure ASCII source the count equals the byte count.
pub(crate) fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let mut scan = Scan::default();
    for (byte, ch) in text.char_indices() {
        if byte >= byte_offset {
            break;
        }
        scan.step(ch, byte, text);
    }
    Position {
        line: scan.line,
        character: scan.utf16_col,
    }
}

/// Map an LSP `Position` (line, UTF-16 character column) to a byte
/// offset into the source text. The inverse of [`byte_offset_to_position`].
pub(crate) fn position_to_byte_offset(text: &str, line: u32, utf16_col: u32) -> Option<usize> {
    let mut scan = Scan::default();
    for (byte, ch) in text.char_indices() {
        if scan.line == line {
            if scan.utf16_col == utf16_col {
                return Some(byte);
            }
            // No byte boundary exists inside an astral scalar's UTF-16
            // surrogate pair. Reject that position instead of snapping it
            // forward to the next scalar.
            if scan.utf16_col > utf16_col {
                return None;
            }
        }
        scan.step(ch, byte, text);
    }
    if scan.line == line && scan.utf16_col == utf16_col {
        Some(text.len())
    } else {
        None
    }
}

/// Convert a byte offset to a tree-sitter `Point` (row, byte column).
/// Columns here are BYTES from the line start — tree-sitter's unit, not
/// LSP's UTF-16 unit.
pub(crate) fn byte_offset_to_point(text: &str, byte_offset: usize) -> Point {
    let mut end = byte_offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = &text[..end];
    Point {
        row: prefix.bytes().filter(|byte| *byte == b'\n').count(),
        column: end - prefix.rfind('\n').map_or(0, |byte| byte + 1),
    }
}

/// Byte offset of the first character of 0-indexed `line`. Offsets past
/// the last line clamp to the end of the text.
pub(crate) fn line_start(text: &str, line: usize) -> usize {
    match line.checked_sub(1) {
        None => 0,
        Some(newlines) => text
            .char_indices()
            .filter(|&(_, ch)| ch == '\n')
            .nth(newlines)
            .map_or(text.len(), |(byte, _)| byte + 1),
    }
}
