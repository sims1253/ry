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
use ry_core::{RParser, SourceFile};

/// Result of discovering and parsing all on-disk R files under a root.
pub(crate) struct IndexOutcome {
    pub files: HashMap<String, Arc<SourceFile>>,
    /// Per-root cap reports; empty when no limit was reached.
    pub truncated: Vec<ry_workspace::TruncationReport>,
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

/// Upper bound on index parse parallelism (see [`index_pool`]).
const MAX_INDEX_THREADS: usize = 8;

/// Bounded rayon pool dedicated to workspace indexing, created lazily on
/// the first index and reused for every re-index (watched-files and
/// configuration changes trigger rescans) so worker threads and their
/// cached parsers survive between scans. Returns `None` when no pool can
/// be built at all (thread exhaustion), in which case callers index
/// serially: indexing must never take the server down.
///
/// The CLI parses on rayon's global pool — its whole process exists to
/// check one workspace and then exit, so using every core is right. The
/// LSP is different: it is a long-lived background process sharing the
/// machine with the editor, and it indexes while the user types.
/// Saturating every core would starve the editor and any other language
/// server, so index parallelism is capped at `min(8,
/// available_parallelism)`: enough to hide parse latency on large
/// workspaces while leaving headroom on big machines. Idle pool threads
/// park (no CPU cost) and each holds one parser, so the pool's steady
/// footprint is at most 8 cached tree-sitter parsers.
fn index_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, MAX_INDEX_THREADS);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("ry-index-{i}"))
            .build()
            .or_else(|error| {
                tracing::warn!(%error, threads, "failed to build index pool; indexing single-threaded");
                // Indexing must never take the server down: degrade to
                // one worker rather than propagate the panic.
                rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .thread_name(|i| format!("ry-index-{i}"))
                    .build()
            })
            .map_err(|error| {
                tracing::warn!(%error, "single-threaded index pool also failed; indexing serially");
                error
            })
            .ok()
    })
    .as_ref()
}

fn parse_paths(paths: &[PathBuf]) -> HashMap<String, Arc<SourceFile>> {
    // Parse in parallel on the bounded index pool, mirroring the CLI's
    // `ry-cli::pipeline::parse_files`: each worker reuses a thread-local
    // `RParser` (tree-sitter parsers are Send but neither Sync nor
    // cheap to construct, so one per worker is the right granularity).
    // Error tolerance matches the previous serial loop: an unreadable
    // file or a parse failure is skipped, never fatal to the index.
    // When no pool could be built, the same per-file logic runs inline.
    let parse_one = |path: &PathBuf| {
        let source = ry_workspace::read_r_source(path).ok()?;
        let path_str = path.to_string_lossy().into_owned();
        let file = parse_with_worker_parser(&path_str, &source)?;
        Some((path_str, Arc::new(file)))
    };
    match index_pool() {
        Some(pool) => pool.install(|| paths.par_iter().filter_map(parse_one).collect()),
        None => paths.iter().filter_map(parse_one).collect(),
    }
}

/// Parse one file with this worker thread's cached parser, constructing
/// it on first use. A construction failure skips this file (and leaves
/// the slot empty so the next file retries) rather than failing the
/// whole index.
fn parse_with_worker_parser(path: &str, source: &str) -> Option<SourceFile> {
    thread_local! {
        static PARSER: std::cell::RefCell<Option<RParser>> =
            const { std::cell::RefCell::new(None) };
    }
    PARSER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let parser = match slot.as_mut() {
            Some(parser) => parser,
            None => match RParser::new() {
                Ok(parser) => slot.insert(parser),
                Err(error) => {
                    tracing::warn!(path, %error, "index parser init failed; skipping file");
                    return None;
                }
            },
        };
        match parser.parse(path, source) {
            Ok(file) => Some(file),
            Err(error) => {
                tracing::debug!(path, %error, "index parse failed; skipping file");
                None
            }
        }
    })
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
        let discovered = index_workspace(dir, &config);

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
        let discovered = index_workspace(dir, &cfg);
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
        let discovered = index_workspace(dir, &config);

        let paths: Vec<&str> = discovered.files.keys().map(String::as_str).collect();
        assert_eq!(paths.len(), 1, "only keep.R: {paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("keep.R")), "keep.R found");
        assert!(
            !paths.iter().any(|p| p.ends_with("skip.R")),
            "target/ must be skipped"
        );
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
        let outcome = index_workspace(dir, &config);
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
        let outcome = index_workspace(dir, &config);

        assert!(
            outcome.truncated.iter().any(|t| t.max_files_hit),
            "max-files cap must be reported"
        );
        assert_eq!(outcome.files.len(), 2, "only 2 files discovered under cap");
    }
}
