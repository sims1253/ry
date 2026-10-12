//! Workspace file discovery and background indexing.
//!
//! Delegates directory discovery to the shared [`ry_workspace`] module so
//! the CLI (`ry check .`) and the LSP use identical eligibility, extension,
//! hidden-directory, symlink, exclude, test-fixture, and bounded-cap rules.
//!
//! Open documents shadow on-disk contents because the editor's buffer
//! is authoritative — a file being edited may have unsaved changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

use rayon::prelude::*;
use ry_config::Config;
use ry_core::SourceFile;

/// Result of discovering and parsing all on-disk R files under a root.
pub(crate) struct IndexOutcome {
    pub files: HashMap<String, Arc<SourceFile>>,
    /// Per-root cap reports; empty when no limit was reached.
    pub truncated: Vec<ry_workspace::TruncationReport>,
}

/// Discover and parse all eligible sources under `walk_root`, honouring
/// `exclude` patterns and bounded caps. `exclude_root` anchors those
/// patterns and `include-build-ignored` at the config's directory, as the
/// CLI does. Returns parsed files plus any cap reports to surface.
pub(crate) fn index_workspace(
    walk_root: &Path,
    exclude_root: Option<&Path>,
    config: &Config,
) -> IndexOutcome {
    let discovery =
        ry_workspace::discover_r_files(walk_root, exclude_root, config, config.check_test_fixtures);
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

/// Upper bound on index parse parallelism (see [`index_pool`]).
const MAX_INDEX_THREADS: usize = 8;

/// Reuse at most eight workers so indexing leaves room for the editor.
/// If thread creation fails, parse inline instead.
fn index_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|&threads| threads > 0)
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
            .min(MAX_INDEX_THREADS);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("ry-index-{i}"))
            .build()
            .inspect_err(|error| {
                tracing::warn!(%error, threads, "failed to build index pool; indexing serially");
            })
            .ok()
    })
    .as_ref()
}

/// Parse every discovered path on the lazily built [`index_pool`], in
/// parallel when the pool is available and serially when it is not. A
/// file that cannot be read or parsed is skipped, never fatal to the index.
fn parse_paths(paths: &[PathBuf]) -> HashMap<String, Arc<SourceFile>> {
    parse_paths_with(paths, index_pool())
}

/// Parse `paths` with an explicit pool, or inline without one.
fn parse_paths_with(
    paths: &[PathBuf],
    pool: Option<&rayon::ThreadPool>,
) -> HashMap<String, Arc<SourceFile>> {
    let parse_one = |path: &PathBuf| match ry_workspace::parse_source_file(path, None) {
        Ok(file) => Some((file.path.clone(), Arc::new(file))),
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "index parse failed; skipping file");
            None
        }
    };
    match pool {
        Some(pool) => pool.install(|| paths.par_iter().filter_map(parse_one).collect()),
        None => paths.iter().filter_map(parse_one).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_r_files() {
        let fixture = ry_testkit::FixtureProject::empty().unwrap();
        let dir = fixture.root();

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
        let discovered = index_workspace(dir, Some(dir), &config);

        let paths: Vec<&str> = discovered.files.keys().map(String::as_str).collect();
        assert_eq!(paths.len(), 3, "a.R, b.r and sub/d.R: {paths:?}");
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
    }

    #[test]
    fn respects_exclude_globs() {
        let fixture = ry_testkit::FixtureProject::empty().unwrap();
        let dir = fixture.root();

        std::fs::write(dir.join("keep.R"), "x <- 1\n").unwrap();

        let excluded = dir.join("vendor");
        std::fs::create_dir_all(&excluded).unwrap();
        std::fs::write(excluded.join("skip.R"), "y <- 2\n").unwrap();

        // Build a config with exclude = ["vendor"]
        let cfg = ry_config::Config {
            exclude: vec!["vendor".to_string()],
            ..Default::default()
        };
        let discovered = index_workspace(dir, Some(dir), &cfg);
        let paths: Vec<&str> = discovered.files.keys().map(String::as_str).collect();

        assert_eq!(paths.len(), 1, "only keep.R: {paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("keep.R")), "keep.R found");
        assert!(
            !paths.iter().any(|p| p.ends_with("skip.R")),
            "vendor/ skipped"
        );
    }

    /// The LSP must skip `target/` directories just like
    /// the CLI, so both modes discover the same file set.
    #[test]
    fn skips_target_directory_like_cli() {
        let fixture = ry_testkit::FixtureProject::empty().unwrap();
        let dir = fixture.root();

        std::fs::write(dir.join("keep.R"), "x <- 1\n").unwrap();

        let target = dir.join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("skip.R"), "y <- 2\n").unwrap();

        let config = Config::default();
        let discovered = index_workspace(dir, Some(dir), &config);

        let paths: Vec<&str> = discovered.files.keys().map(String::as_str).collect();
        assert_eq!(paths.len(), 1, "only keep.R: {paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("keep.R")), "keep.R found");
        assert!(
            !paths.iter().any(|p| p.ends_with("skip.R")),
            "target/ must be skipped"
        );
    }

    /// The serial fallback used when no index pool can be built must
    /// still land every parseable file in the map.
    #[test]
    fn serial_index_without_pool_parses_every_file() {
        let fixture = ry_testkit::FixtureProject::empty().unwrap();
        let dir = fixture.root();
        let paths: Vec<PathBuf> = (0..5)
            .map(|i| {
                let path = dir.join(format!("s{i}.R"));
                std::fs::write(&path, format!("y_{i} <- {i}\n")).unwrap();
                path
            })
            .collect();

        let files = parse_paths_with(&paths, None);

        assert_eq!(files.len(), paths.len(), "every file must be indexed");
        for path in &paths {
            assert!(
                files.contains_key(&path.to_string_lossy().into_owned()),
                "{} missing",
                path.display()
            );
        }
    }

    /// Parallel parsing on the bounded pool must land every parseable
    /// file in the map regardless of how rayon splits the work across
    /// workers (each worker owns its own thread-local parser).
    #[test]
    fn parallel_index_parses_every_file() {
        let fixture = ry_testkit::FixtureProject::empty().unwrap();
        let dir = fixture.root();
        let count = 33;
        for i in 0..count {
            std::fs::write(dir.join(format!("f{i:02}.R")), format!("x_{i} <- {i}\n")).unwrap();
        }
        let config = Config::default();
        let outcome = index_workspace(dir, Some(dir), &config);
        assert_eq!(outcome.files.len(), count, "every file must be indexed");
        for i in 0..count {
            assert!(
                outcome.files.contains_key(
                    &dir.join(format!("f{i:02}.R"))
                        .to_string_lossy()
                        .into_owned()
                ),
                "f{i:02}.R missing"
            );
        }
    }

    /// Truncated state must be exposed to tests.
    #[test]
    fn exposes_truncation_when_max_files_hit() {
        let fixture = ry_testkit::FixtureProject::empty().unwrap();
        let dir = fixture.root();

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
        let outcome = index_workspace(dir, Some(dir), &config);

        assert!(
            outcome.truncated.iter().any(|t| t.max_files_hit),
            "max-files cap must be reported"
        );
        assert_eq!(outcome.files.len(), 2, "only 2 files discovered under cap");
    }
}
