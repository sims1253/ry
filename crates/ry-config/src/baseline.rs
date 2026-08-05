//! Baseline and severity-filter helpers shared between the CLI and the
//! LSP server.
//!
//! These types were previously embedded in `ry-cli`'s `main.rs` where
//! they were unreachable from the language server.

use crate::Config;
use miette::{IntoDiagnostic, Result};
use ry_checker::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Build a [`ry_checker::SeverityFilter`] from the `error`, `warn`, and
/// `ignore` lists of a [`Config`]. The resulting filter is what the
/// checker applies to raw diagnostics before they are displayed.
pub fn build_filter(
    error: &[String],
    warn: &[String],
    ignore: &[String],
) -> ry_checker::SeverityFilter {
    let mut f = ry_checker::SeverityFilter::default();
    for e in error {
        f.add_error(e);
    }
    for w in warn {
        f.add_warn(w);
    }
    for i in ignore {
        f.add_ignore(i);
    }
    f
}

/// Convenience: build a [`ry_checker::SeverityFilter`] directly from a
/// [`Config`].
pub fn filter_from_config(cfg: &Config) -> ry_checker::SeverityFilter {
    build_filter(&cfg.error, &cfg.warn, &cfg.ignore)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Baseline {
    pub version: u32,
    pub entries: Vec<BaselineEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub path: String,
    pub code: String,
    pub message: String,
    pub count: usize,
}

pub fn load_baseline(path: &Path) -> Result<Baseline> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| miette::miette!("could not read baseline {}: {error}", path.display()))?;
    let baseline: Baseline = serde_json::from_str(&contents)
        .map_err(|error| miette::miette!("could not parse baseline {}: {error}", path.display()))?;
    if baseline.version != 1 {
        return Err(miette::miette!(
            "unsupported baseline version {} in {}; expected 1",
            baseline.version,
            path.display()
        ));
    }
    Ok(baseline)
}

pub fn diagnostic_path(path: &str, repo_root: Option<&Path>) -> String {
    let path = Path::new(path);
    let root = repo_root
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    root.as_deref()
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

pub fn write_baseline_file(
    path: &Path,
    diagnostics: &[Diagnostic],
    repo_root: Option<&Path>,
) -> Result<()> {
    let mut counts = std::collections::BTreeMap::new();
    for diagnostic in diagnostics {
        *counts
            .entry((
                diagnostic_path(&diagnostic.path, repo_root),
                diagnostic.code.to_string(),
                diagnostic.message.clone(),
            ))
            .or_insert(0usize) += 1;
    }
    let entries = counts
        .into_iter()
        .map(|((path, code, message), count)| BaselineEntry {
            path,
            code,
            message,
            count,
        })
        .collect();
    let baseline = Baseline {
        version: 1,
        entries,
    };
    let contents = serde_json::to_string_pretty(&baseline).into_diagnostic()?;
    std::fs::write(path, format!("{contents}\n"))
        .map_err(|error| miette::miette!("could not write baseline {}: {error}", path.display()))
}

pub fn subtract_baseline(
    diagnostics: &mut Vec<Diagnostic>,
    baseline: &Baseline,
    repo_root: Option<&Path>,
) {
    let mut remaining: HashMap<(String, String, String), usize> = baseline
        .entries
        .iter()
        .map(|entry| {
            (
                (
                    entry.path.clone(),
                    entry.code.clone(),
                    entry.message.clone(),
                ),
                entry.count,
            )
        })
        .collect();
    diagnostics.retain(|diagnostic| {
        let path = diagnostic_path(&diagnostic.path, repo_root);
        let key = (
            path,
            diagnostic.code.to_string(),
            diagnostic.message.clone(),
        );
        match remaining.get_mut(&key) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        }
    });
}
