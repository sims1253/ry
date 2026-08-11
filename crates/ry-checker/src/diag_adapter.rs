//! Adapters between ry internal types and ry-diagnostics vocabulary.
//!
//! Plan 38-W2: ry-diagnostics owns context-free primitives (TextSize, TextRange,
//! RuleId). ry owns R-specific logic and conversion. This module bridges the two.

pub use ry_diagnostics::{Confidence as DiagConfidence, Severity as DiagSeverity};

/// Convert ry-core Severity to ry-diagnostics Severity.
pub fn to_diag_severity(s: ry_core::Severity) -> DiagSeverity {
    match s {
        ry_core::Severity::Error => DiagSeverity::Error,
        ry_core::Severity::Warning => DiagSeverity::Warning,
        ry_core::Severity::Info => DiagSeverity::Information,
    }
}

/// Convert ry-core Confidence to ry-diagnostics Confidence.
pub fn to_diag_confidence(c: ry_core::Confidence) -> DiagConfidence {
    match c {
        ry_core::Confidence::High => DiagConfidence::High,
        ry_core::Confidence::Medium => DiagConfidence::Medium,
        ry_core::Confidence::Low => DiagConfidence::Low,
    }
}
