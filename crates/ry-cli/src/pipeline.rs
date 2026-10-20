//! Shared analysis for `check`, `dump-types`, and `dump-facts`.
//! Commands own their output and failure policies.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use ry_config as config;
use ry_core::declarations::DeclarationRecord;

/// One package group's parsed files and the context they are checked in.
pub(crate) struct CheckInput {
    pub files: Vec<(String, Arc<ry_core::SourceFile>)>,
    pub user_stubs: Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
    pub workspace: ry_workspace::WorkspaceContext,
}

impl CheckInput {
    /// The files of `parsed` at `indices`, checked in `workspace`.
    pub(crate) fn new(
        parsed: &[Arc<ry_core::SourceFile>],
        indices: &[usize],
        user_stubs: &Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
        workspace: ry_workspace::WorkspaceContext,
    ) -> Self {
        Self {
            files: indices
                .iter()
                .map(|&index| (parsed[index].path.clone(), Arc::clone(&parsed[index])))
                .collect(),
            user_stubs: Arc::clone(user_stubs),
            workspace,
        }
    }

    fn into_project(self, records: Vec<DeclarationRecord>) -> ry_checker::Project {
        let mut project = ry_checker::Project::new();
        project.set_declaration_records(records);
        project.set_user_stubs(self.user_stubs);
        ry_checker::frontend::set_workspace_context(&mut project, self.workspace);
        for (path, file) in self.files {
            project.add_file_arc(path, file);
        }
        project
    }
}

pub(crate) use ry_checker::frontend::adopted_records;

/// Run a one-shot project check with workspace metadata and adopted
/// declaration records.
pub(crate) fn check_project(
    input: CheckInput,
    records: Vec<DeclarationRecord>,
) -> Vec<(String, Vec<ry_checker::Diagnostic>)> {
    let mut project = input.into_project(records);
    let mut diagnostics = project.check();
    ry_checker::append_declaration_diagnostics(&mut diagnostics, project.declaration_findings());
    diagnostics
}

pub(crate) struct CapturedFacts {
    pub scopes: Vec<(String, Vec<ry_checker::ScopeRecord>)>,
    pub references: Vec<(String, ry_checker::ReferenceFacts)>,
}

/// Capture scope snapshots and optional reference evidence in one project check.
pub(crate) fn check_project_with_facts_capture(
    input: CheckInput,
    references: bool,
    records: Vec<DeclarationRecord>,
) -> CapturedFacts {
    let mut project = input.into_project(records);
    project.enable_scope_capture();
    if references {
        project.enable_reference_capture();
    }
    project.check();
    CapturedFacts {
        scopes: project.take_scope_records(),
        references: project.take_reference_facts(),
    }
}

/// Load package stubs from every `--typeshed` directory. Warnings are
/// surfaced; an unreadable directory keeps the run going.
pub(crate) fn load_user_stubs(
    dirs: &[PathBuf],
) -> Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>> {
    let loaded = ry_workspace::load_stub_dirs(dirs);
    for warning in loaded.warnings {
        eprintln!("ry: warning: {warning}");
    }
    Arc::new(loaded.stubs)
}

pub(crate) fn sort_and_deduplicate_paths(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}

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
pub(crate) use ry_workspace::SourceError as ParseError;

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
/// Every failure is passed to `on_failure` as it happens; the callback
/// reports it and picks the action. `Skip`ped files are dropped from
/// the result. The first failure (in input order) whose action is
/// `Abort` is returned in `Err` — and is the only one the caller must
/// report, because the callback stays silent for files it aborts on.
///
/// Each file's native path is on
/// [`ry_core::SourceFile::native_or_display_path`]; `overlay` substitutes one
/// in-memory source.
pub(crate) fn parse_files(
    paths: &[PathBuf],
    overlay: Option<&SourceOverlay>,
    on_failure: impl Fn(&Path, &ParseError) -> FailureAction + Sync,
) -> Result<Vec<Arc<ry_core::SourceFile>>, ParseFailure> {
    use rayon::prelude::*;
    size_rayon_pool();
    let outcomes: Vec<_> = paths
        .par_iter()
        .map(|path| {
            let overlay = overlay
                .filter(|source| source.path == *path)
                .map(|source| source.bytes.as_slice());
            match ry_workspace::parse_source_file(path, overlay) {
                Ok(file) => Some(Ok(Arc::new(file))),
                Err(error) => match on_failure(path, &error) {
                    FailureAction::Skip => None,
                    FailureAction::Abort => Some(Err(ParseFailure {
                        path: path.clone(),
                        error,
                    })),
                },
            }
        })
        .collect();
    outcomes.into_iter().flatten().collect()
}

/// One in-memory source substituted at its logical path. The same parser,
/// decoder, and downstream project analysis are used as for disk sources.
pub(crate) struct SourceOverlay {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// Resolve parsed files into per-package checker inputs, shared by
/// `ry check` and `ry dump-types` so the two commands can never disagree
/// about library scoping or resolution roots.
///
/// Each DESCRIPTION root becomes its own group (see
/// [`ry_workspace::resolve_grouped`]). A group without a package root resolves
/// against the first entry of `fallback_roots` that is set — `ry check`
/// passes the config root, `dump-types` passes `--project-root` and then
/// the config root — and finally against the working directory.
pub(crate) fn resolve_groups(
    parsed: &[Arc<ry_core::SourceFile>],
    cfg: &config::Config,
    user_stubs: &BTreeMap<String, ry_typeshed::Typeshed>,
    fallback_roots: &[Option<&Path>],
) -> miette::Result<Vec<ry_workspace::ResolvedGroup>> {
    let fallback = fallback_roots
        .iter()
        .flatten()
        .copied()
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let files: Vec<&ry_core::SourceFile> = parsed.iter().map(AsRef::as_ref).collect();
    ry_workspace::resolve_grouped(&files, cfg, user_stubs, &fallback)
        .into_iter()
        .map(|group| group.map_err(|error| miette::miette!(error)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn check_project_applies_the_workspace_context() {
        // `is_null(x)` narrows `x` away from NULL only when rlang is
        // attached, so the trailing `x()` is an RY070 exactly when the
        // workspace context does not reach the checker.
        let run = |workspace: ry_workspace::WorkspaceContext| -> usize {
            let file = ry_core::RParser::new()
                .unwrap()
                .parse(
                    "test.R",
                    "x <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n",
                )
                .unwrap();
            let input = CheckInput {
                files: vec![("test.R".to_string(), Arc::new(file))],
                user_stubs: Arc::new(BTreeMap::new()),
                workspace,
            };
            check_project(input, Vec::new())
                .iter()
                .flat_map(|(_, diagnostics)| diagnostics)
                .filter(|diagnostic| diagnostic.code == "RY070")
                .count()
        };
        assert!(run(Default::default()) > 0);
        let mut attached = ry_workspace::WorkspaceContext::default();
        attached.attached_packages.insert("rlang".to_string());
        assert_eq!(run(attached), 0);
    }

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
            let result = parse_files(&paths, None, |_, error| {
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
