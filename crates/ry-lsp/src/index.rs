//! Workspace file discovery and background indexing.
//!
//! Delegates directory discovery to the shared [`ry_workspace`] module so
//! the CLI (`ry check .`) and the LSP use identical eligibility, extension,
//! hidden-directory, symlink, exclude, test-fixture, and bounded-cap rules
//! (P36-W7 / issue #48).
//!
//! Open documents shadow on-disk contents because the editor's buffer
//! is authoritative — a file being edited may have unsaved changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ry_config::Config;
use ry_core::{RParser, SourceFile};

/// Result of discovering and parsing all on-disk R files under a root.
pub(crate) struct IndexOutcome {
    pub files: HashMap<String, Arc<SourceFile>>,
    /// Per-root cap reports; empty when no limit was reached.
    pub truncated: Vec<ry_workspace::TruncationReport>,
}

/// Discover all eligible R files under `root` using the shared bounded
/// discovery module and read their source text.
///
/// Paths are returned as absolute strings (matching the LSP's `uri_to_path`
/// convention). Used by unit tests; production code calls
/// [`index_workspace`] which returns parsed files and cap reports.
#[cfg(test)]
pub(crate) fn discover_r_files(root: &Path, config: &Config) -> Vec<(String, String)> {
    let result =
        ry_workspace::discover_r_files(root, Some(root), config, config.check_test_fixtures);
    result
        .files
        .into_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(&path).ok()?;
            Some((path.to_string_lossy().into_owned(), source))
        })
        .collect()
}

/// Discover and parse all eligible R files under `root`, honouring
/// `exclude` patterns and bounded caps. Returns parsed files plus
/// any cap reports for the caller to surface as warnings.
pub(crate) fn index_workspace(root: &Path, config: &Config) -> IndexOutcome {
    let discovery =
        ry_workspace::discover_r_files(root, Some(root), config, config.check_test_fixtures);
    let files = parse_paths(&discovery.files);
    IndexOutcome {
        files,
        truncated: if discovery.truncated.any_hit() {
            vec![discovery.truncated]
        } else {
            Vec::new()
        },
    }
}

fn parse_paths(paths: &[PathBuf]) -> HashMap<String, Arc<SourceFile>> {
    let mut parsed = HashMap::new();
    let mut parser = match RParser::new() {
        Ok(p) => p,
        Err(_) => return parsed,
    };
    for path in paths {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Ok(file) = parser.parse(&path.to_string_lossy(), &source) {
            parsed.insert(path.to_string_lossy().into_owned(), Arc::new(file));
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_r_files() {
        let dir = std::env::temp_dir().join(format!("ry_index_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Create some .R files
        std::fs::write(dir.join("a.R"), "x <- 1\n").unwrap();
        std::fs::write(dir.join("b.r"), "y <- 2\n").unwrap();
        std::fs::write(dir.join("c.txt"), "not R\n").unwrap();

        // Create a subdirectory
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("d.R"), "z <- 3\n").unwrap();

        // Create a hidden directory (should be skipped)
        let hidden = dir.join(".hidden");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(hidden.join("e.R"), "hidden <- TRUE\n").unwrap();

        let config = Config::default();
        let discovered = discover_r_files(&dir, &config);

        let paths: Vec<&str> = discovered.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.iter().any(|p| p.ends_with("a.R")), "a.R found");
        assert!(paths.iter().any(|p| p.ends_with("b.r")), "b.r found");
        assert!(paths.iter().any(|p| p.ends_with("d.R")), "d.R in sub found");
        assert!(
            !paths.iter().any(|p| p.ends_with("c.txt")),
            "c.txt excluded"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("e.R")),
            "hidden dir skipped"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn respects_exclude_globs() {
        let dir = std::env::temp_dir().join(format!("ry_index_excl_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("keep.R"), "x <- 1\n").unwrap();

        let excluded = dir.join("vendor");
        std::fs::create_dir_all(&excluded).unwrap();
        std::fs::write(excluded.join("skip.R"), "y <- 2\n").unwrap();

        // Build a config with exclude = ["vendor"]
        let cfg = ry_config::Config {
            exclude: vec!["vendor".to_string()],
            ..Default::default()
        };
        let discovered = discover_r_files(&dir, &cfg);
        let paths: Vec<&str> = discovered.iter().map(|(p, _)| p.as_str()).collect();

        assert!(paths.iter().any(|p| p.ends_with("keep.R")), "keep.R found");
        assert!(
            !paths.iter().any(|p| p.ends_with("skip.R")),
            "vendor/ skipped"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parses_discovered_files() {
        let dir = std::env::temp_dir().join(format!("ry_index_parse_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("a.R"), "f <- function(x) x + 1\n").unwrap();
        std::fs::write(dir.join("b.R"), "g <- function() f(2)\n").unwrap();

        let config = Config::default();
        let outcome = index_workspace(&dir, &config);

        assert_eq!(outcome.files.len(), 2, "two files parsed");
        assert!(outcome.files.keys().any(|p| p.ends_with("a.R")));
        assert!(outcome.files.keys().any(|p| p.ends_with("b.R")));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// P36-W7 (#48): the LSP must skip `target/` directories just like
    /// the CLI, so both modes discover the same file set.
    #[test]
    fn skips_target_directory_like_cli() {
        let dir = std::env::temp_dir().join(format!("ry_index_target_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("keep.R"), "x <- 1\n").unwrap();

        let target = dir.join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("skip.R"), "y <- 2\n").unwrap();

        let config = Config::default();
        let discovered = discover_r_files(&dir, &config);

        let paths: Vec<&str> = discovered.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.iter().any(|p| p.ends_with("keep.R")), "keep.R found");
        assert!(
            !paths.iter().any(|p| p.ends_with("skip.R")),
            "target/ must be skipped (P36-W7)"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// P36-W7 (#48): truncated state must be exposed to tests.
    #[test]
    fn exposes_truncation_when_max_files_hit() {
        let dir = std::env::temp_dir().join(format!("ry_index_cap_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Write more files than the cap allows.
        for i in 0..5 {
            std::fs::write(dir.join(format!("file_{i}.R")), "x <- 1\n").unwrap();
        }

        let config = ry_config::Config {
            index: ry_config::IndexConfig {
                max_files: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let outcome = index_workspace(&dir, &config);

        assert!(
            outcome.truncated.iter().any(|t| t.max_files_hit),
            "max-files cap must be reported"
        );
        assert_eq!(outcome.files.len(), 2, "only 2 files discovered under cap");

        std::fs::remove_dir_all(&dir).ok();
    }
}
