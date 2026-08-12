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
        for fn_name in all_names(catalog) {
            if let Some(sem) = catalog.lookup(&fn_name) {
                merged.register(&fn_name, sem.clone());
            }
        }
    }
    merged
}

/// Get all function names from a catalog.
fn all_names(catalog: &InMemoryCatalog) -> Vec<String> {
    // Collect from all packages.
    let mut names: Vec<String> = Vec::new();
    // The InMemoryCatalog doesn't expose all keys directly,
    // so we iterate through known packages.
    for pkg in [
        "base",
        "stats",
        "utils",
        "graphics",
        "grDevices",
        "methods",
        "rlang",
        "dplyr",
        "tidyr",
        "purrr",
        "ggplot2",
        "readr",
        "stringr",
        "forcats",
        "tibble",
        "lubridate",
        "broom",
        "testthat",
        "shiny",
        "jsonlite",
        "httr",
    ] {
        for name in catalog.package_functions(pkg) {
            names.push(name.to_string());
        }
    }
    names
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
}
