//! Inlay hints from types captured at assignment sites.

use ry_core::presentation::{compact_hint, expanded_type};
use ry_core::{RType, Span};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, InlayHintTooltip};

use crate::positions::byte_offset_to_position;

/// Render known assignment types at the end of their original identifier.
/// Unknown types provide no useful annotation, so omit them.
pub(super) fn collect_inlay_hints(types: &[(Span, RType)], text: &str) -> Vec<InlayHint> {
    types
        .iter()
        .filter(|(_, ty)| !matches!(ty.mode, ry_core::types::Mode::Opaque))
        .map(|(span, ty)| {
            let label = compact_hint(ty);
            let details = expanded_type(ty);
            let mut tooltip = format!("Inferred facts: {}", details.text);
            if details.truncated {
                tooltip.push_str(
                    "\nExpanded view truncated; use `ry dump-facts` for structured facts.",
                );
            }
            InlayHint {
                position: byte_offset_to_position(text, span.end),
                label: InlayHintLabel::String(format!(": {}", label.text)),
                kind: Some(InlayHintKind::TYPE),
                tooltip: Some(InlayHintTooltip::String(tooltip)),
                padding_left: Some(true),
                padding_right: None,
                text_edits: None,
                data: None,
            }
        })
        .collect()
}
