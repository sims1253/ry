//! Workspace file discovery and background indexing.
//!
//! Discovers `.R`/`.r` files under workspace roots, parses them in the
//! background, and stores the results for `publish_diagnostics` to merge
//! with open documents. This closes the file-set gap between the CLI
//! (`ry check .`) and the editor: the LSP now sees all project files,
//! not just open ones (Plan 33 W4).
//!
//! Open documents shadow on-disk contents because the editor's buffer
//! is authoritative — a file being edited may have unsaved changes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ry_config::Config;
use ry_core::{RParser, SourceFile};

/// Walk a workspace root, discovering all `.R`/`.r` files that are not
/// excluded by the configured glob patterns. Returns `(path, source_text)`
/// pairs for the caller to parse.
///
/// Paths are returned as absolute strings (matching the LSP's `uri_to_path`
/// convention). The walk is breadth-first and skips excluded directories
/// early to avoid descending into `node_modules`, `.git`, etc.
pub(crate) fn discover_r_files(root: &Path, config: &Config) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    let excludes = ry_config::Excludes::from_config(config);

    while let Some(dir) = queue.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            // Check excludes before descending.
            if !ry_workspace::is_file_eligible_with_excludes(&path, root, &excludes) {
                continue;
            }

            if path.is_dir() && !entry.file_type().is_ok_and(|ft| ft.is_symlink()) {
                // Skip hidden directories (.git, .Rproj.user, etc).
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        continue;
                    }
                }
                queue.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("R") || ext.eq_ignore_ascii_case("r"))
            {
                if let Ok(source) = std::fs::read_to_string(&path) {
                    let abs = path.to_string_lossy().into_owned();
                    results.push((abs, source));
                }
            }
        }
    }

    results
}

/// Parse all discovered files into `SourceFile`s. Each file gets its own
/// `RParser` (the parser is not `Sync`). Files that fail to parse are
/// silently skipped — a syntax error in an unopened file should not
/// prevent indexing the rest of the project.
pub(crate) fn parse_disk_files(files: &[(String, String)]) -> HashMap<String, Arc<SourceFile>> {
    let mut parsed = HashMap::new();
    let mut parser = match RParser::new() {
        Ok(p) => p,
        Err(_) => return parsed,
    };

    for (path, source) in files {
        if let Ok(file) = parser.parse(path, source) {
            parsed.insert(path.clone(), Arc::new(file));
        }
    }

    parsed
}

/// Discover and parse all `.R`/`.r` files under `root`, honouring `excludes`.
/// Convenience wrapper combining `discover_r_files` + `parse_disk_files`.
pub(crate) fn index_workspace(root: &Path, config: &Config) -> HashMap<String, Arc<SourceFile>> {
    let discovered = discover_r_files(root, config);
    parse_disk_files(&discovered)
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
        let parsed = index_workspace(&dir, &config);

        assert_eq!(parsed.len(), 2, "two files parsed");
        assert!(parsed.keys().any(|p| p.ends_with("a.R")));
        assert!(parsed.keys().any(|p| p.ends_with("b.R")));

        std::fs::remove_dir_all(&dir).ok();
    }
}
