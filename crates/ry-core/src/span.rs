//! Span: a byte range in a source file, with line/column for diagnostics.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    /// Pre-resolved line (0-indexed) and column. The column is the
    /// **byte** offset of the span start within its line (i.e. tree-sitter's
    /// `start_position().column`), NOT a character column. Renderers that
    /// need a character column for display should convert per-line via
    /// `ry_core::parser::byte_col_to_char_col`.
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self {
            start,
            end,
            line,
            col,
        }
    }
}
