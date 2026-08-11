//! Interactive query types for editor intelligence.
//!
//! P38-W7: These types represent hover, completion, and signature data
//! in analysis-domain terms, independent of LSP protocol types.

/// Hover information for a symbol at a position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverInfo {
    /// The symbol name.
    pub name: String,
    /// The inferred type as a human-readable string.
    pub type_string: String,
    /// Byte offset of the hovered identifier.
    pub start: u32,
    /// End byte offset.
    pub end: u32,
}

/// A completion item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// The text to insert.
    pub label: String,
    /// The kind of completion (function, variable, etc.).
    pub kind: CompletionKind,
    /// Optional detail (e.g., function signature).
    pub detail: Option<String>,
}

/// Kind of a completion item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Function,
    Variable,
    Parameter,
    Keyword,
    Package,
}

/// Signature help for a function call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureInfo {
    /// Function label (e.g., "fn(x, y, ...)").
    pub label: String,
    /// Parameter names.
    pub parameters: Vec<String>,
    /// Active parameter index.
    pub active_parameter: Option<usize>,
}

/// An inlay hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    /// Byte offset where the hint should appear.
    pub position: u32,
    /// The hint text.
    pub label: String,
    /// Kind of inlay hint.
    pub kind: InlayHintKind,
}

/// Kind of inlay hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlayHintKind {
    Type,
    Parameter,
}
