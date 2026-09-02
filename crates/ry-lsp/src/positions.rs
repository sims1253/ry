//! Line/column conversions for the LSP layer — one home for every
//! text-position walk in this crate.
//!
//! All conversion semantics in ry-lsp reduce to one line-walking rule
//! (the [`Scan`] core below: lines are delimited by `\n` only) plus the
//! column unit each caller needs:
//!
//! * **UTF-16 code units** — the LSP wire format
//!   (`Position.character`, counted by [`byte_offset_to_position`] and
//!   [`position_to_byte_offset`]). The `\r` of a CRLF terminator is not
//!   a column character; non-ASCII (including astral-plane) text
//!   resolves correctly.
//! * **bytes** — tree-sitter `Point` columns
//!   ([`byte_offset_to_point`]) and the line-start byte offsets
//!   ([`line_start`]) used to bound whole-line text edits.
//!
//! The position conversions share the [`Scan`] core and differ only
//! in the column unit they report; [`line_start`] skips the scan,
//! sharing only the newline-only line rule.

use ry_core::Point;
use tower_lsp::lsp_types::Position;

/// The line/column state of a left-to-right scan: the shared
/// line-walking core every helper below advances. Lines are delimited
/// by `\n` only; a lone `\r` is an ordinary column character.
struct Scan {
    /// 0-based line of the next unconsumed character.
    line: u32,
    /// Byte offset where the current line starts.
    line_start: usize,
    /// UTF-16 code units from the current line start up to the scan
    /// position (LSP semantics: the `\r` of a CRLF terminator is not a
    /// column character).
    utf16_col: u32,
}

impl Scan {
    fn new() -> Self {
        Self {
            line: 0,
            line_start: 0,
            utf16_col: 0,
        }
    }

    /// Consume the character at byte offset `byte`, updating the line
    /// and column state.
    fn step(&mut self, ch: char, byte: usize, text: &str) {
        if ch == '\n' {
            self.line += 1;
            self.line_start = byte + 1;
            self.utf16_col = 0;
        } else if !is_crlf_cr(ch, byte, text) {
            self.utf16_col += utf16_len(ch) as u32;
        }
    }
}

/// Whether the `ch` at `byte` is the `\r` of a CRLF pair (a line
/// terminator fragment, not a column character).
fn is_crlf_cr(ch: char, byte: usize, text: &str) -> bool {
    ch == '\r' && text.as_bytes().get(byte + 1) == Some(&b'\n')
}

/// Number of UTF-16 code units a Unicode scalar value encodes to: 1 for
/// the Basic Multilingual Plane, 2 for astral-plane characters (which
/// become a surrogate pair).
fn utf16_len(ch: char) -> usize {
    if (ch as u32) >= 0x10000 { 2 } else { 1 }
}

/// Map a byte offset into the source text to an LSP `Position`
/// (0-indexed line, 0-indexed character column).
///
/// The LSP spec defines `Position.character` as a UTF-16 code-unit
/// offset. This helper counts UTF-16 code units (each BMP character is
/// 1 unit; astral-plane characters -- emoji, rare CJK -- are 2). For
/// pure ASCII source the count equals the byte count.
pub(crate) fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let mut scan = Scan::new();
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
    let mut scan = Scan::new();
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
    let offset = byte_offset.min(text.len());
    let mut scan = Scan::new();
    for (byte, ch) in text.char_indices() {
        if byte >= offset {
            break;
        }
        scan.step(ch, byte, text);
    }
    Point {
        row: scan.line as usize,
        column: offset - scan.line_start,
    }
}

/// Byte offset of the first character of 0-indexed `line`. Offsets past
/// the last line clamp to the end of the text.
pub(crate) fn line_start(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut current = 0usize;
    for (byte, ch) in text.char_indices() {
        if ch == '\n' {
            current += 1;
            if current == line {
                return byte + 1;
            }
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_cr_is_an_ordinary_utf16_column() {
        // A CR that is not part of a CRLF pair neither starts a new line
        // nor vanishes from the column count: it is one column wide.
        let text = "ab\rcd";
        assert_eq!(
            byte_offset_to_position(text, 3),
            Position {
                line: 0,
                character: 3
            }
        );
        assert_eq!(position_to_byte_offset(text, 0, 3), Some(3));

        // Contrast: the `\r` of a CRLF terminator is not a column
        // character — the byte after the pair is column 0 of line 1.
        let crlf = "a\r\nb";
        assert_eq!(
            byte_offset_to_position(crlf, 3),
            Position {
                line: 1,
                character: 0
            }
        );
    }

    #[test]
    fn position_to_byte_offset_rejects_mid_surrogate_position() {
        // 'a' is column 0, '😀' occupies columns 1-2, 'b' is column 3.
        // Column 2 falls between the surrogate pair: no byte boundary
        // exists there, so the inverse mapping rejects it instead of
        // snapping forward to 'b'.
        let text = "a😀b";
        assert_eq!(position_to_byte_offset(text, 0, 1), Some(1));
        assert_eq!(position_to_byte_offset(text, 0, 2), None);
        assert_eq!(position_to_byte_offset(text, 0, 3), Some(5));
    }
}
