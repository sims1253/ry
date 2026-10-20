//! Background indexing of on-disk sources. Discovery is the shared
//! [`ry_workspace`] walk, so the CLI and the LSP admit the same files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

use rayon::prelude::*;
use ry_config::Config;
use ry_core::SourceFile;

pub(crate) struct IndexOutcome {
    pub files: HashMap<String, Arc<SourceFile>>,
    /// Set when a discovery cap was reached.
    pub truncated: Option<ry_workspace::TruncationReport>,
}

/// Discover and parse every eligible source under `walk_root`. Excludes and
/// `include-build-ignored` anchor at `exclude_root`, the config's directory,
/// as in the CLI.
pub(crate) fn index_workspace(
    walk_root: &Path,
    exclude_root: Option<&Path>,
    config: &Config,
) -> IndexOutcome {
    let discovery =
        ry_workspace::discover_r_files(walk_root, exclude_root, config, config.check_test_fixtures);
    IndexOutcome {
        files: parse_paths(&discovery.files),
        truncated: discovery.truncated.any_hit().then_some(discovery.truncated),
    }
}

/// The server indexes while the user types, so it leaves cores for the
/// editor instead of using every core like the CLI.
const MAX_INDEX_THREADS: usize = 8;

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
                tracing::warn!(%error, threads, "failed to build index pool; using the global pool");
            })
            .ok()
    })
    .as_ref()
}

/// Parse `paths` in parallel. A file that cannot be read or parsed is
/// skipped, never fatal to the index.
fn parse_paths(paths: &[PathBuf]) -> HashMap<String, Arc<SourceFile>> {
    let parse = || {
        paths
            .par_iter()
            .filter_map(|path| match ry_workspace::parse_source_file(path, None) {
                Ok(file) => Some((file.path.clone(), Arc::new(file))),
                Err(error) => {
                    tracing::debug!(path = %path.display(), %error, "index parse failed; skipping file");
                    None
                }
            })
            .collect()
    };
    match index_pool() {
        Some(pool) => pool.install(parse),
        None => parse(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            outcome.truncated.is_some_and(|t| t.max_files_hit),
            "max-files cap must be reported"
        );
        assert_eq!(outcome.files.len(), 2, "only 2 files discovered under cap");
    }
}
