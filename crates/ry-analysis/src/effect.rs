//! Effect interpreter — one runtime semantics path.
//!
//! P39-W5: This module provides a single entry point for interpreting
//! function semantics. It replaces the scattered field reads across
//! checker infer modules.

use crate::catalog::{Evaluation, FlowEffect, FunctionSemantics, ReturnRule, SemanticCatalog};

// == Issue #49: Catalog query seam for signature resolution ==

/// Query result for a function call's semantics.
#[derive(Debug, Clone)]
pub struct CallSemantics {
    /// The resolved semantics, or None if the function is unknown.
    pub semantics: Option<FunctionSemantics>,
    /// Whether the function is known to the catalog.
    pub known: bool,
}

/// Look up semantics for a function call.
///
/// This is the single seam (issue #49). The checker calls this instead
/// of reading individual schema fields from scattered locations.
pub fn lookup_call<C: SemanticCatalog + ?Sized>(catalog: &C, function: &str) -> CallSemantics {
    match catalog.lookup(function) {
        Some(sem) => CallSemantics {
            semantics: Some(sem.clone()),
            known: true,
        },
        None => CallSemantics {
            semantics: None,
            known: false,
        },
    }
}

// == Issue #41: One NSE/defusing encoding ==

/// Determine whether a function captures arguments as unevaluated promises.
///
/// This replaces three overlapping encodings:
/// 1. Hardcoded DEFUSING_CALLS list in semantic_lists.rs
/// 2. Stub-driven eval mode in typeshed
/// 3. Inline promise-capture checks in the infer modules
pub fn is_defusing(sem: &FunctionSemantics) -> bool {
    matches!(sem.evaluation, Evaluation::PromiseCapture { .. })
}

/// Determine whether a function uses data-mask semantics.
pub fn is_data_mask(sem: &FunctionSemantics) -> bool {
    matches!(sem.evaluation, Evaluation::DataMask { .. })
}

// == Issue #40: Semantic flags via the catalog ==

/// Check if a function narrows types as a predicate.
pub fn is_predicate(sem: &FunctionSemantics) -> bool {
    matches!(sem.flow_effect, FlowEffect::Predicate { .. })
}

/// Check if a function asserts and may halt execution.
pub fn is_assertion(sem: &FunctionSemantics) -> bool {
    matches!(sem.flow_effect, FlowEffect::Assertion { .. })
}

/// Check if a function never returns.
pub fn is_no_return(sem: &FunctionSemantics) -> bool {
    matches!(sem.flow_effect, FlowEffect::NoReturn)
}

/// Get the return rule for a function.
pub fn return_rule(sem: &FunctionSemantics) -> &ReturnRule {
    &sem.return_rule
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::*;

    #[test]
    fn lookup_unknown_function() {
        let cat = InMemoryCatalog::new();
        let result = lookup_call(&cat, "unknown::fn");
        assert!(!result.known);
        assert!(result.semantics.is_none());
    }

    #[test]
    fn lookup_known_function() {
        let mut cat = InMemoryCatalog::new();
        cat.register("base::length", FunctionSemantics::default());
        let result = lookup_call(&cat, "base::length");
        assert!(result.known);
        assert!(result.semantics.is_some());
    }

    #[test]
    fn defusing_classification() {
        let eager = FunctionSemantics::default();
        assert!(!is_defusing(&eager));

        let defusing = FunctionSemantics {
            evaluation: Evaluation::PromiseCapture {
                defusing: DefusingKind::Enquo,
            },
            ..Default::default()
        };
        assert!(is_defusing(&defusing));
    }

    #[test]
    fn predicate_classification() {
        let normal = FunctionSemantics::default();
        assert!(!is_predicate(&normal));

        let pred = FunctionSemantics {
            flow_effect: FlowEffect::Predicate {
                target: PredicateTarget::FirstArg,
            },
            ..Default::default()
        };
        assert!(is_predicate(&pred));
    }
}
