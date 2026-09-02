//! Baseline and severity-filter helpers shared between the CLI and the
//! LSP server.

use miette::{IntoDiagnostic, Result};
use ry_core::BaselineDiagnostic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub version: u32,
    pub entries: Vec<BaselineEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    let current_dir = std::env::current_dir().ok();
    let root = repo_root.or(current_dir.as_deref());
    root.and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

pub fn write_baseline_file<D: BaselineDiagnostic>(
    path: &Path,
    diagnostics: &[D],
    repo_root: Option<&Path>,
) -> Result<()> {
    let mut counts = std::collections::BTreeMap::new();
    for diagnostic in diagnostics {
        *counts
            .entry((
                diagnostic_path(diagnostic.path(), repo_root),
                diagnostic.code().to_string(),
                diagnostic.message().to_string(),
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

pub fn subtract_baseline<D: BaselineDiagnostic>(
    diagnostics: &mut Vec<D>,
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
        let path = diagnostic_path(diagnostic.path(), repo_root);
        let key = (
            path,
            diagnostic.code().to_string(),
            diagnostic.message().to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `BaselineDiagnostic` carrier; baseline logic keys on
    /// (path, code, message) only, so spans are unnecessary here.
    struct TestDiag {
        path: String,
        code: String,
        message: String,
    }

    impl BaselineDiagnostic for TestDiag {
        fn path(&self) -> &str {
            &self.path
        }
        fn code(&self) -> &str {
            &self.code
        }
        fn message(&self) -> &str {
            &self.message
        }
    }

    fn diag(path: &str, code: &str) -> TestDiag {
        TestDiag {
            path: path.to_string(),
            code: code.to_string(),
            message: "same message".to_string(),
        }
    }

    #[test]
    fn baseline_subtract_removes_matching_entries() {
        let baseline = Baseline {
            version: 1,
            entries: vec![BaselineEntry {
                path: "R/a.R".to_string(),
                code: "RY010".to_string(),
                message: "test".to_string(),
                count: 1,
            }],
        };

        let mut diags = vec![TestDiag {
            path: "R/a.R".to_string(),
            code: "RY010".to_string(),
            message: "test".to_string(),
        }];

        subtract_baseline(&mut diags, &baseline, None);
        assert!(diags.is_empty(), "baseline should remove the diagnostic");
    }

    #[test]
    fn baseline_round_trip_suppresses_existing_but_not_new_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("baseline.json");
        let existing = diag("a.R", "RY010");
        write_baseline_file(&path, std::slice::from_ref(&existing), Some(temp.path())).unwrap();
        let baseline = load_baseline(&path).unwrap();
        let mut diagnostics = vec![existing, diag("a.R", "RY030")];
        subtract_baseline(&mut diagnostics, &baseline, Some(temp.path()));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "RY030");
    }

    #[test]
    fn baseline_counts_absorb_only_the_recorded_occurrences() {
        let baseline = Baseline {
            version: 1,
            entries: vec![BaselineEntry {
                path: "a.R".to_string(),
                code: "RY010".to_string(),
                message: "same message".to_string(),
                count: 2,
            }],
        };
        let mut diagnostics = vec![
            diag("a.R", "RY010"),
            diag("a.R", "RY010"),
            diag("a.R", "RY010"),
        ];
        subtract_baseline(&mut diagnostics, &baseline, None);
        assert_eq!(diagnostics.len(), 1);
    }
}
