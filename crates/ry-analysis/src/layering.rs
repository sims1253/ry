//! Catalog layering — merge official and user catalogs.
//!
//! P39-W6: Custom typesheds use the same validator/compiler as the
//! official catalog. User entries override official entries with the
//! same fully-qualified name.

use crate::catalog::{InMemoryCatalog, SemanticCatalog};

/// Layer catalogs with priority: later layers override earlier ones.
///
/// Usage: `layer_catalogs(&[base_catalog, project_catalog, user_catalog])`
/// User entries take precedence over project entries, which take
/// precedence over base entries.
pub fn layer_catalogs(catalogs: &[InMemoryCatalog]) -> InMemoryCatalog {
    let mut merged = InMemoryCatalog::new();
    for catalog in catalogs {
        // Each catalog in order; later registrations override earlier.
        for fn_name in catalog.function_names() {
            if let Some(sem) = catalog.lookup(fn_name) {
                merged.register(fn_name, sem.clone());
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::*;

    #[test]
    fn user_overrides_base() {
        let mut base = InMemoryCatalog::new();
        base.register(
            "base::fn",
            FunctionSemantics {
                return_rule: ReturnRule::Fixed("base_type".to_string()),
                ..Default::default()
            },
        );

        let mut user = InMemoryCatalog::new();
        user.register(
            "base::fn",
            FunctionSemantics {
                return_rule: ReturnRule::Fixed("user_type".to_string()),
                ..Default::default()
            },
        );

        let layered = layer_catalogs(&[base, user]);
        let sem = layered.lookup("base::fn").unwrap();
        assert_eq!(
            sem.return_rule,
            ReturnRule::Fixed("user_type".to_string()),
            "user catalog should override base"
        );
    }

    /// Layering previously enumerated a hardcoded list of 21 package names,
    /// so any entry outside that list was silently dropped. Every registered
    /// entry must survive layering regardless of its package.
    #[test]
    fn layering_preserves_entries_from_unlisted_packages() {
        let mut base = InMemoryCatalog::new();
        for name in ["base::fn", "data.table::fread", "zoo::rollmean", "bare_fn"] {
            base.register(
                name,
                FunctionSemantics {
                    return_rule: ReturnRule::Fixed(format!("{name}_type")),
                    ..Default::default()
                },
            );
        }

        let layered = layer_catalogs(&[base]);

        assert_eq!(layered.function_count(), 4, "no entry may be dropped");
        for name in ["base::fn", "data.table::fread", "zoo::rollmean", "bare_fn"] {
            let sem = layered
                .lookup(name)
                .unwrap_or_else(|| panic!("{name} was dropped by layering"));
            assert_eq!(sem.return_rule, ReturnRule::Fixed(format!("{name}_type")));
        }
    }
}
