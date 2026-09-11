//! Front half of the analysis pipeline, shared by `ry check` and
//! `ry dump-types`: config discovery, parallel parsing, per-package
//! grouping, and workspace-context construction. Both commands feed off
//! these helpers so their file sets, resolution roots, and workspace
//! models cannot drift apart.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use ry_config as config;

/// Discover a ry.toml by walking up from `search_start`.
///
/// A missing config is not an error: the defaults come back with no
/// root. A present-but-malformed config IS an error: it is printed here
/// and the failure code is returned for the caller to propagate, so the
/// user notices the typo rather than silently running with defaults.
pub(crate) fn discover_config(
    search_start: &Path,
) -> Result<(Option<PathBuf>, config::Config), ExitCode> {
    match config::Config::discover(search_start) {
        Ok(Some((path, cfg))) => {
            tracing::debug!(config = %path.display(), "loaded ry.toml");
            Ok((path.parent().map(PathBuf::from), cfg))
        }
        Ok(None) => Ok((None, config::Config::default())),
        Err(e) => {
            eprintln!("ry: {}", e);
            Err(ExitCode::FAILURE)
        }
    }
}

/// Why one input file could not be parsed.
#[derive(Debug)]
pub(crate) enum ParseError {
    /// Reading the file failed.
    Read(std::io::Error),
    /// The parser rejected the source.
    Parse(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Read(error) => write!(f, "{error}"),
            ParseError::Parse(message) => write!(f, "{message}"),
        }
    }
}

/// One failed input file, for the caller to report.
#[derive(Debug)]
pub(crate) struct ParseFailure {
    pub path: PathBuf,
    pub error: ParseError,
}

/// What a command does with a failed file.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FailureAction {
    /// Drop the file and keep going.
    Skip,
    /// Stop the whole run; the caller reports the failure.
    Abort,
}

/// Size rayon's global pool before the first parallel work starts
/// (all CLI parallelism funnels through [`parse_files`]). The check
/// pipeline alternates parallel phases (parsing, diagnostic emission)
/// with serial ones (workspace resolution, fixpoint refinement), so a
/// worker per core spends the serial phases spinning and waking: on a
/// 24-core machine the default pool burns ~4x the system time of a
/// 12-worker pool for the same wall time. Capping at 12 keeps
/// throughput flat while cutting CPU and system time on large machines;
/// an explicit `RAYON_NUM_THREADS` still wins.
///
/// Called lazily so subcommands that never touch rayon (`--version`,
/// `--help`, completions) do not pay for spawning workers. Idempotent:
/// rayon's global registry is process-wide and `build_global` fails on
/// a second attempt, so a `Once` guards the (warning-free) first call.
fn size_rayon_pool() {
    use rayon::ThreadPoolBuilder;
    use std::sync::Once;
    static SIZED: Once = Once::new();
    SIZED.call_once(|| {
        if std::env::var_os("RAYON_NUM_THREADS").is_some() {
            return;
        }
        let workers = std::thread::available_parallelism()
            .map_or(8, std::num::NonZeroUsize::get)
            .min(12);
        if let Err(error) = ThreadPoolBuilder::new().num_threads(workers).build_global() {
            eprintln!("ry: warning: could not size the thread pool: {error}");
        }
    });
}

/// Parse every path in parallel on rayon's pool, in input order.
///
/// Each rayon thread reuses an `RParser` across files and runs to avoid
/// loading the grammar for every file.
///
/// Every failure is passed to `on_failure` as it happens; the callback
/// reports it and picks the action. `Skip`ped files are dropped from
/// the result. The first failure (in input order) whose action is
/// `Abort` is returned in `Err` — and is the only one the caller must
/// report, because the callback stays silent for files it aborts on.
pub(crate) fn parse_files(
    paths: &[PathBuf],
    on_failure: impl Fn(&Path, &ParseError) -> FailureAction + Sync,
) -> Result<Vec<Arc<ry_core::SourceFile>>, ParseFailure> {
    use rayon::prelude::*;
    size_rayon_pool();
    let outcomes: Vec<_> = paths
        .par_iter()
        .map(|path| match parse_one(path) {
            Ok(file) => Some(Ok(file)),
            Err(failure) => match on_failure(&failure.path, &failure.error) {
                FailureAction::Skip => None,
                FailureAction::Abort => Some(Err(failure)),
            },
        })
        .collect();
    outcomes.into_iter().flatten().collect()
}

/// Read and parse one file on the calling thread, using that thread's
/// parser from the pool (see [`parse_files`]).
fn parse_one(path: &Path) -> Result<Arc<ry_core::SourceFile>, ParseFailure> {
    thread_local! {
        static PARSER: std::cell::RefCell<Option<ry_core::RParser>> =
            const { std::cell::RefCell::new(None) };
    }
    let src = match ry_workspace::read_r_source(path) {
        Ok(src) => src,
        Err(error) => {
            return Err(ParseFailure {
                path: path.to_path_buf(),
                error: ParseError::Read(error),
            });
        }
    };
    let path_str = path.to_string_lossy().to_string();
    let file = PARSER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let parser = match slot.as_mut() {
            Some(parser) => parser,
            None => match ry_core::RParser::new() {
                Ok(parser) => slot.insert(parser),
                // A worker whose parser cannot initialize must not panic the
                // whole check: report the file as unparseable and leave the
                // slot empty so the next file retries, matching the LSP's
                // index tolerance.
                Err(error) => return Err(error.to_string()),
            },
        };
        parser
            .parse(&path_str, &src)
            .map_err(|message| message.to_string())
    });
    file.map(Arc::new).map_err(|message| ParseFailure {
        path: path.to_path_buf(),
        error: ParseError::Parse(message),
    })
}

/// Nearest ancestor directory (starting at the path itself for
/// directories, at the parent for files) holding a DESCRIPTION file.
fn enclosing_package_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    start
        .ancestors()
        .find(|ancestor| ancestor.join("DESCRIPTION").is_file())
        .map(Path::to_path_buf)
}

/// Group path strings by enclosing package root, keeping each group's
/// input indices in ascending order. Each R package is a separate
/// library scope: pooling multiple package roots into one project lets
/// top-level bindings and inferred functions leak between namespaces,
/// which can both hide real RY010 findings and activate the wrong NSE
/// model. Non-package scripts share the `None` group so ordinary
/// multi-file workflows keep their source()-style visibility.
pub(crate) fn group_by_package_root<'a, I>(paths: I) -> BTreeMap<Option<PathBuf>, Vec<usize>>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut groups: BTreeMap<Option<PathBuf>, Vec<usize>> = BTreeMap::new();
    // The ancestor DESCRIPTION walk is identical for every file in one
    // directory, so run it once per distinct directory instead of once
    // per file.
    let mut root_cache: HashMap<Option<&'a Path>, Option<PathBuf>> = HashMap::new();
    for (index, path) in paths.into_iter().enumerate() {
        let path = Path::new(path);
        let key = path.parent();
        let root = root_cache
            .entry(key)
            .or_insert_with(|| enclosing_package_root(path))
            .clone();
        groups.entry(root).or_default().push(index);
    }
    groups
}

/// One per-package group, ready for the checker: the group's files and
/// workspace context wrapped as a `CheckInput`, plus the degraded-scope
/// notes the command reports in its own voice.
pub(crate) struct ResolvedGroup {
    pub resolution_root: PathBuf,
    pub check_input: crate::check::CheckInput,
    pub degraded_scopes: Vec<(PathBuf, &'static str)>,
}

/// Resolve parsed files into per-package checker inputs, shared by
/// `ry check` and `ry dump-types` so the two commands can never disagree
/// about library scoping or resolution roots.
///
/// Each DESCRIPTION root becomes its own group (see
/// [`group_by_package_root`]). A group without a package root resolves
/// against the first entry of `fallback_roots` that is set — `ry check`
/// passes the config root, `dump-types` passes `--project-root` and then
/// the config root — and finally against the working directory. The
/// workspace context's `degraded_scopes` are split out of the
/// `CheckInput` because each command reports the precision loss
/// differently: check summarizes them in its summary line, dump-types
/// prints one note per scope on stderr to keep stdout's JSON clean.
pub(crate) fn resolve_groups(
    parsed: &[Arc<ry_core::SourceFile>],
    cfg: &config::Config,
    user_stubs: &Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
    fallback_roots: &[Option<&Path>],
) -> miette::Result<Vec<ResolvedGroup>> {
    let groups = group_by_package_root(parsed.iter().map(|file| file.path.as_str()));
    let mut resolved = Vec::with_capacity(groups.len());
    for (group_root, indices) in &groups {
        let resolution_root = group_root
            .clone()
            .or_else(|| {
                fallback_roots
                    .iter()
                    .flatten()
                    .copied()
                    .next()
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        // A relative file such as R/main.R can find DESCRIPTION at the
        // empty ancestor path. That ancestor is the working directory.
        let resolution_root = if resolution_root.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            resolution_root
        };
        let mut package_scope = ry_workspace::resolve_workspace_context(
            &resolution_root,
            cfg,
            ry_workspace::ResolutionEnvironment {
                files: indices
                    .iter()
                    .map(|index| parsed[*index].as_ref())
                    .collect(),
                user_stubs,
            },
        )
        .map_err(|error| miette::miette!(error))?;
        let analysis_files = indices
            .iter()
            .map(|index| {
                let parsed_file = &parsed[*index];
                (parsed_file.path.clone(), Arc::clone(parsed_file))
            })
            .collect();
        let degraded_scopes = std::mem::take(&mut package_scope.degraded_scopes);
        resolved.push(ResolvedGroup {
            resolution_root,
            check_input: crate::check::CheckInput {
                files: analysis_files,
                user_stubs: Arc::clone(user_stubs),
                workspace: package_scope,
            },
            degraded_scopes,
        });
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn parallel_parsing_preserves_order_and_reports_every_failure() {
        let dir = tempfile::tempdir().unwrap();
        let paths =
            ["z.R", "missing-first.R", "missing-second.R", "a.R"].map(|name| dir.path().join(name));
        for index in [0, 3] {
            std::fs::write(&paths[index], "x <- 1\n").unwrap();
        }
        for action in [FailureAction::Skip, FailureAction::Abort] {
            let failures = AtomicUsize::new(0);
            let result = parse_files(&paths, |_, error| {
                assert!(matches!(error, ParseError::Read(_)));
                failures.fetch_add(1, Ordering::Relaxed);
                action
            });
            assert_eq!(failures.load(Ordering::Relaxed), 2);
            match action {
                FailureAction::Skip => assert_eq!(
                    result
                        .unwrap()
                        .iter()
                        .map(|file| Path::new(&file.path))
                        .collect::<Vec<_>>(),
                    [&paths[0], &paths[3]],
                ),
                FailureAction::Abort => assert_eq!(result.unwrap_err().path, paths[1]),
            }
        }
    }
}
