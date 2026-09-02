//! Diagnostic vocabulary — severity, confidence, and baseline traits.
//!
//! These types are defined at the lowest layer so that `ry-config` and
//! `ry-workspace` can depend on them without pulling in the full checker.
//! The checker (`ry-checker`) re-exports them for backward compatibility.

use serde::{Deserialize, Serialize};

/// Severity level for a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// Confidence level for a diagnostic finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Demote by one step: High → Medium → Low.
    pub fn demote(self) -> Self {
        match self {
            Self::High => Self::Medium,
            Self::Medium | Self::Low => Self::Low,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Trait for types that participate in baseline subtraction.
/// Exposes the minimal fields needed to match a diagnostic against
/// baseline entries (path, code, message).
pub trait BaselineDiagnostic {
    fn path(&self) -> &str;
    fn code(&self) -> &str;
    fn message(&self) -> &str;
}

/// External-binding sentinel marking unenumerable serialized bindings.
/// The `\0` prefix keeps it out of the R identifier namespace.
pub const SERIALIZED_BINDINGS_UNENUMERABLE: &str = "\0serialized:unenumerable";

/// R foreign-function-interface primitives.
/// Their first argument is a native routine entry-point symbol, not a variable.
/// `.Internal` is deliberately absent: its first argument is a call
/// expression, not a bare entry-point symbol.
pub const FFI_PRIMITIVES: &[&str] = &[".Call", ".C", ".Fortran", ".External", ".External2"];
