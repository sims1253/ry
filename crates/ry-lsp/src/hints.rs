//! Inlay hints from types captured at assignment sites.

use ry_core::{RType, Span};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::positions::byte_offset_to_position;

/// Render known assignment types at the end of their original identifier.
/// Unknown types provide no useful annotation, so omit them.
pub(super) fn collect_inlay_hints(types: &[(Span, RType)], text: &str) -> Vec<InlayHint> {
    types
        .iter()
        .filter(|(_, ty)| !matches!(ty.mode, ry_core::types::Mode::Opaque))
        .map(|(span, ty)| InlayHint {
            position: byte_offset_to_position(text, span.end),
            label: InlayHintLabel::String(format!(": {ty}")),
            kind: Some(InlayHintKind::TYPE),
            tooltip: None,
            padding_left: Some(true),
            padding_right: None,
            text_edits: None,
            data: None,
        })
        .collect()
}
