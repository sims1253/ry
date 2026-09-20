//! Unified diagnostics query — one entry point for the CLI.
//!
//! The caller supplies parsed files and workspace context; the module
//! returns diagnostics. This file also owns the `ry check` command's
//! orchestration (`run_check` and its one-pass driver `run_check_once`):
//! config/CLI flag merging, file discovery, watch mode, output rendering,
//! and the exit-code policy. main.rs keeps only argument parsing and
//! dispatch.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::SystemTime;

use clap::ArgMatches;
use clap::parser::ValueSource;
use miette::Result;

use ry_config as config;

use crate::CheckArgs;
use crate::pipeline;

/// Input for a unified diagnostics check.
pub struct CheckInput {
    /// Parsed source files as (path, SourceFile) tuples.
    pub files: Vec<(String, Arc<ry_core::SourceFile>)>,
    /// User typeshed stubs.
    pub user_stubs: Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Workspace context (package metadata, bindings).
    pub workspace: ry_workspace::WorkspaceContext,
}

impl CheckInput {
    fn into_project(self) -> ry_checker::Project {
        let mut project = ry_checker::Project::new();
        let workspace = self.workspace;
        project.set_loaded(workspace.attached_packages);
        project.set_bare_loaded(workspace.bare_bindings);
        project.set_user_stubs(self.user_stubs);
        project.set_external_bindings(workspace.external_bindings);
        project.set_imported_from(workspace.imported_bindings);
        project.set_external_s3_methods(workspace.s3_methods);
        project.set_load_bindings(workspace.load_bindings);
        for (path, file) in self.files {
            project.add_file_arc(path, file);
        }
        project
    }
}

/// Run a one-shot project check with workspace metadata.
pub fn check_project(input: CheckInput) -> Vec<(String, Vec<ry_checker::Diagnostic>)> {
    input.into_project().check()
}

/// Run the same one-shot check, additionally snapshotting every file's
/// lexical scopes (top level plus each walked function body).
///
/// The pipeline is identical to [`check_project`] -- same workspace
/// metadata, shared fixpoint, one pass -- so captured types match what a
/// `check` run infers. Returns one `(path, records)` entry per file, in
/// input order. Diagnostics are computed as usual but discarded: the
/// dump consumer (`ry dump-types`) treats diagnostics as irrelevant to
/// its exit code and output.
pub fn check_project_with_scope_capture(
    input: CheckInput,
) -> Vec<(String, Vec<ry_checker::ScopeRecord>)> {
    check_project_with_facts_capture(input, false).scopes
}

pub(crate) struct CapturedFacts {
    pub scopes: Vec<(String, Vec<ry_checker::ScopeRecord>)>,
    pub references: Vec<(String, ry_checker::ReferenceFacts)>,
}

/// Capture scope snapshots and optional reference evidence in one project check.
pub(crate) fn check_project_with_facts_capture(
    input: CheckInput,
    references: bool,
) -> CapturedFacts {
    let mut project = input.into_project();
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

/// Drive `ry check`: merge the CLI flags with `ry.toml`, discover the
/// R files, check them once, and keep re-checking in watch mode.
pub(crate) fn run_check(
    args: CheckArgs,
    cli_verbose: u8,
    cli_quiet: u8,
    check_matches: Option<&ArgMatches>,
) -> Result<ExitCode> {
    let CheckArgs {
        paths,
        error,
        warn,
        ignore,
        typeshed,
        error_on_warning,
        exit_zero,
        output_format,
        color,
        watch,
        statistics,
        explain_files,
        write_baseline,
        baseline,
        min_confidence,
    } = args;

    // Config discovery is anchored at the first input path (itself for a
    // directory, its parent for a file — `Config::discover` applies that
    // rule) or at the working directory when no paths were given, the
    // same anchor `ry dump-types` uses. Owned so watch mode can
    // re-discover after `paths` moves into `search_roots` below.
    let search_start: PathBuf = paths.first().cloned().unwrap_or_else(|| PathBuf::from("."));

    let (config_root, base_cfg) = match pipeline::discover_config(&search_start) {
        Ok(found) => found,
        Err(code) => return Ok(code),
    };

    // Forward `None` for scalars the CLI did not set explicitly, so the
    // config file's value wins. The overrides are retained for watch
    // mode, which re-merges them over every reloaded `ry.toml` (#530).
    let m = check_matches;
    let baseline_from_cli = flag_set(m, "baseline");

    let overrides = config::CliOverrides {
        error,
        warn,
        ignore,
        typeshed,
        baseline,
        error_on_warning: flag_set(m, "error_on_warning").then_some(error_on_warning),
        exit_zero: flag_set(m, "exit_zero").then_some(exit_zero),
        output_format: flag_set(m, "output_format").then_some(output_format.to_string()),
        verbose: cli_verbose,
        quiet: cli_quiet,
    };
    let cfg = base_cfg.merge_cli(overrides.clone());

    // Regeneration snapshots what the run reports as it stands, so the
    // configured baseline is neither loaded nor subtracted: subtracting
    // first would empty the file on an unchanged project and resurrect
    // every accepted finding on the next plain check (#484). This
    // mirrors at the config level the clap conflict between
    // `--write-baseline` and `--baseline` (`baseline_from_cli` is
    // therefore unreachable here); a genuinely fixed finding still drops
    // out because the snapshot is rebuilt from the current diagnostics.
    let baseline = if write_baseline.is_some() {
        None
    } else {
        match cfg.baseline.as_deref() {
            Some(path) => match config::load_baseline(path) {
                Ok(value) => Some(value),
                Err(error) if baseline_from_cli => return Err(error),
                Err(error) => {
                    eprintln!("ry: warning: {error}");
                    None
                }
            },
            None => None,
        }
    };

    // Initialize after config discovery so ry.toml verbosity takes effect.
    init_tracing(cfg.verbose, cfg.quiet);

    let format = ry_checker::format::OutputFormat::parse(&cfg.output_format).ok_or_else(|| {
        miette::miette!(
            "unknown --output-format `{}`; expected one of: full, concise, json, github, gitlab, junit",
            cfg.output_format
        )
    })?;
    let color_choice = color;
    let color = color.enabled(format);
    let filter = ry_checker::filter_from_config(&cfg);
    let user_stubs = load_user_stubs(&cfg.typeshed);

    // Collect the initial file set via the shared bounded discovery
    // module (issue #48). CLI and LSP use the same eligibility,
    // extension, hidden-directory, symlink, exclude, and test-fixture rules.
    let search_roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths
    };

    // Every requested input root must exist (#485): a missing path used
    // to fall into the directory branch of discovery, whose failed
    // `read_dir` was swallowed, so a typo'd CI path checked as clean.
    // `ry dump-types` already rejects missing inputs this way. The run
    // aborts before checking anything — a partial check that never
    // mentions the miss is what made the bug dangerous.
    let missing: Vec<&PathBuf> = search_roots.iter().filter(|root| !root.exists()).collect();
    if !missing.is_empty() {
        for root in &missing {
            eprintln!("ry: {}: no such file or directory", root.display());
        }
        // Keep the machine-readable stream well-formed: an empty report,
        // same as the empty-discovery branch below.
        print!(
            "{}",
            render_diagnostics(&[], format, &HashMap::new(), color)
        );
        return Ok(discovery_exit_code(&cfg));
    }

    let scan = rescan(
        &search_roots,
        config_root.as_deref(),
        &cfg,
        true,
        explain_files,
    );
    report_read_errors(&scan.read_errors);
    let mut all_paths = scan.paths;

    // A readable-but-empty discovery result enters the watch loop with
    // zero files instead of exiting: the loop's rescan already detects
    // membership growth, so the first created `.R` file triggers a
    // re-check (#529). The read-error branch below still fails the run
    // in watch mode (#485): an unreadable root is a discovery failure,
    // not a quiet empty set. Non-watch behavior is unchanged.
    if all_paths.is_empty() && !(watch && scan.read_errors.is_empty()) {
        // An empty discovery result still needs a complete machine-readable report.
        print!(
            "{}",
            render_diagnostics(&[], format, &HashMap::new(), color)
        );
        if scan.read_errors.is_empty() {
            let roots = search_roots
                .iter()
                .map(|root| root.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!("ry: no .R / .r files found in {roots}");
            return Ok(ExitCode::SUCCESS);
        }
        // Nothing was discovered because the walk could not read a root;
        // the errors above already say why. The informational "no files
        // found" note would mislabel that I/O failure as an intentional
        // empty set.
        return Ok(discovery_exit_code(&cfg));
    }

    // The reloadable inputs of every pass. In watch mode the loop below
    // refreshes them whenever a non-R input changes (#530); the file set
    // stays a parameter of `run_check_once` because watch iterations
    // change it too.
    let mut state = WatchState::new(
        &search_start,
        config_root,
        cfg,
        filter,
        format,
        color_choice,
        user_stubs,
        baseline,
        overrides,
        baseline_from_cli,
    );

    let min_confidence = min_confidence.into();
    let mut result = run_check_once(&all_paths, &state.check_context(min_confidence))?;
    // Directories the initial scan could not read fail the run like
    // parse errors do, even when every discovered file checks clean
    // (#485).
    result.discovery_errors = scan.read_errors.len();
    if let Some(path) = write_baseline.as_deref() {
        config::write_baseline_file(path, &result.diagnostics, state.config_root())?;
    }
    result.print_summary(state.format(), statistics);

    if !watch {
        return Ok(result.exit_code(state.config()));
    }
    if !state.format().is_human() {
        eprintln!("ry: --watch requires the full or concise output format");
        return Ok(ExitCode::FAILURE);
    }

    eprintln!(
        "ry: watching {} file(s) for changes (Ctrl+C to stop)...",
        all_paths.len()
    );
    let mut stamps: HashMap<PathBuf, SystemTime> = HashMap::new();
    sync_stamps(&all_paths, &mut stamps);
    state.sync_static_stamps();
    state.sync_meta_stamps(&search_roots, &all_paths);

    let poll_interval = std::time::Duration::from_millis(500);
    loop {
        std::thread::sleep(poll_interval);

        // Reload the config-anchored inputs first so the rescan below
        // already runs under a changed discovery config (new excludes,
        // fixture flags, index caps). Every touch between two polls
        // coalesces into one reload and one re-check, the same debouncing
        // the R-file mtime loop below provides.
        let inputs_changed = state.poll_static_inputs();

        // Re-scan for new/deleted files via shared bounded discovery.
        // Truncation was already reported on the initial scan, so the
        // poll keeps stderr quiet.
        let current = rescan(
            &search_roots,
            state.config_root(),
            state.config(),
            false,
            false,
        );

        // Poll the package-metadata dependencies AFTER the rescan: the
        // dependency set is derived from the discovered paths, so it must
        // follow a config reload's effect on discovery, and a file set
        // change and the metadata paths it admits are observed by one
        // poll instead of a lagging second one. The snapshot records the
        // values as they stand before the re-check below reads them, so
        // a metadata edit landing DURING the check still differs on the
        // next poll rather than being swallowed by a fresh post-check
        // read. The derivation itself runs only when an input moved
        // (config reload, file-set change) — an idle poll stats the
        // cached candidates as-is.
        let membership_changed = current.paths != all_paths;
        let meta_changed = state.poll_meta_inputs(
            &search_roots,
            &current.paths,
            inputs_changed || membership_changed,
        );

        // Check for any file modification or file set change.
        let mut changed = inputs_changed || meta_changed || membership_changed;
        if !changed {
            for p in &current.paths {
                if let Ok(meta) = std::fs::metadata(p)
                    && let Ok(mtime) = meta.modified()
                {
                    let prev = stamps.get(p).copied();
                    if prev != Some(mtime) {
                        changed = true;
                        stamps.insert(p.clone(), mtime);
                        break;
                    }
                }
            }
        }

        if changed {
            all_paths = current.paths;
            // A root that stopped being readable dropped files out of
            // the set; say so instead of letting them vanish silently.
            report_read_errors(&current.read_errors);
            // Re-sync stamps for any new files.
            sync_stamps(&all_paths, &mut stamps);
            // Clear screen for a clean view of the new diagnostics.
            // Using ANSI escape sequences rather than `clear` command
            // for portability (no external process spawn).
            eprint!("\x1b[2J\x1b[H");
            let result = run_check_once(&all_paths, &state.check_context(min_confidence))?;
            result.print_summary(state.format(), statistics);
        }
    }
}

/// The reloadable inputs of one watch session: everything a check pass
/// reads *besides* the R file set. Watch mode polls the mtimes of the
/// files these inputs come from and re-derives the affected state when
/// one changes, so mid-watch edits take effect without a restart (#530).
/// The inputs split into two classes with different refresh disciplines:
///
/// * Static, config-anchored inputs (`ry.toml` candidates, the effective
///   baseline, stub files under the typeshed dirs): fixed by the CLI
///   roots and the merged config. A change reloads the
///   config-derived state.
/// * Package-metadata dependencies (DESCRIPTION/NAMESPACE along the
///   ancestor chains of the roots and of the discovered R files, each
///   chain stopping at the first absorbing ancestor — see
///   [`package_meta_paths`]): the candidate set is cached and
///   re-derived only when an input to the derivation moves (the file
///   set, the discovery config) or an observed change may have moved an
///   absorbing boundary; every other poll stats the cached candidates
///   as-is. A change needs no config reload — the re-check itself
///   re-reads the metadata — only a pass.
///
/// The LSP classifies the same inputs as resolution changes
/// (`did_change_watched_files`); the CLI mirrors that classification
/// with mtime polling instead of file events.
struct WatchState {
    /// Anchor for config re-discovery (first CLI path, or `.`).
    search_start: PathBuf,
    /// Directory containing the active `ry.toml`, if one was found.
    /// Doubles as the baseline-relative repo root and the resolution
    /// fallback, exactly like the one-shot path's `config_root`.
    config_root: Option<PathBuf>,
    /// Merged config (disk config + CLI overrides), filter, stubs, and
    /// baseline of the current pass.
    cfg: config::Config,
    filter: ry_checker::SeverityFilter,
    format: ry_checker::format::OutputFormat,
    color_choice: crate::ColorChoice,
    user_stubs: Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
    baseline: Option<config::Baseline>,
    /// Retained CLI overrides, re-merged over every reloaded disk
    /// config so flags keep winning over `ry.toml` edits.
    overrides: config::CliOverrides,
    /// Whether the effective baseline came from `--baseline`: startup
    /// aborts on its load errors, and reloads report them loudly too.
    baseline_from_cli: bool,
    /// Last-good policy (#530): a broken `ry.toml` or baseline
    /// mid-watch keeps the previous inputs with one warning per episode
    /// instead of killing the session or spamming every poll. Cleared
    /// when the file parses again, with a recovery note.
    config_broken: bool,
    baseline_broken: bool,
    /// (path, mtime) snapshot of the config-anchored inputs; `None` is a
    /// missing path, so creation and deletion trigger like edits do.
    static_stamps: Vec<(PathBuf, Option<SystemTime>)>,
    /// Derived package-metadata dependency paths of the current pass:
    /// the sorted candidate set [`package_meta_paths`] returned for the
    /// current roots, file set, and discovery config. Cached across
    /// polls because the set can only move when one of those inputs
    /// moves — an idle poll stats these paths instead of re-deriving
    /// them (a HashSet rebuild plus two joins per candidate).
    meta_paths: Vec<PathBuf>,
    /// (path, mtime) snapshot of the package-metadata dependencies
    /// derived from the current roots and discovered file set. `None` is
    /// a missing path, so creating a nearer DESCRIPTION or a NAMESPACE
    /// for a package that had none registers like an edit.
    meta_stamps: Vec<(PathBuf, Option<SystemTime>)>,
}

impl WatchState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        search_start: &std::path::Path,
        config_root: Option<PathBuf>,
        cfg: config::Config,
        filter: ry_checker::SeverityFilter,
        format: ry_checker::format::OutputFormat,
        color_choice: crate::ColorChoice,
        user_stubs: Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
        baseline: Option<config::Baseline>,
        overrides: config::CliOverrides,
        baseline_from_cli: bool,
    ) -> Self {
        Self {
            search_start: search_start.to_path_buf(),
            config_root,
            cfg,
            filter,
            format,
            color_choice,
            user_stubs,
            baseline,
            overrides,
            baseline_from_cli,
            config_broken: false,
            baseline_broken: false,
            static_stamps: Vec::new(),
            meta_paths: Vec::new(),
            meta_stamps: Vec::new(),
        }
    }

    fn config(&self) -> &config::Config {
        &self.cfg
    }

    fn config_root(&self) -> Option<&std::path::Path> {
        self.config_root.as_deref()
    }

    fn format(&self) -> ry_checker::format::OutputFormat {
        self.format
    }

    /// Borrow the current inputs as one pass's context. Rebuilt per
    /// pass because a reload may have replaced everything it borrows.
    fn check_context(&self, min_confidence: ry_checker::Confidence) -> CheckContext<'_> {
        CheckContext {
            filter: &self.filter,
            format: self.format,
            resolution_config: &self.cfg,
            user_stubs: Arc::clone(&self.user_stubs),
            color: self.color_choice.enabled(self.format),
            baseline: self.baseline.as_ref(),
            repo_root: self.config_root.as_deref(),
            min_confidence,
        }
    }

    /// Record the current snapshot of the config-anchored inputs.
    fn sync_static_stamps(&mut self) {
        self.static_stamps = snapshot_static_inputs(
            &config_candidates(&self.search_start),
            self.cfg.baseline.as_deref(),
            &self.cfg.typeshed,
        );
    }

    /// Derive the package-metadata dependency set together with its
    /// snapshot in one step: the single derive/snapshot pairing, shared
    /// by the initial sync ([`WatchState::sync_meta_stamps`]) and the
    /// poll's `rederive` path so the two cannot drift apart.
    fn derive_meta_state(
        &self,
        search_roots: &[PathBuf],
        discovered: &[PathBuf],
    ) -> (Vec<PathBuf>, Vec<(PathBuf, Option<SystemTime>)>) {
        let paths = package_meta_paths(search_roots, discovered, self.config_root());
        let stamps = snapshot_meta_paths(&paths);
        (paths, stamps)
    }

    /// Derive the package-metadata dependency paths for `discovered`
    /// (the file set the next pass will check) and the CLI roots, and
    /// record their snapshot.
    fn sync_meta_stamps(&mut self, search_roots: &[PathBuf], discovered: &[PathBuf]) {
        (self.meta_paths, self.meta_stamps) = self.derive_meta_state(search_roots, discovered);
    }

    /// Compare the config-anchored inputs against the snapshot; on any
    /// difference reload the affected state, refresh the stamp set (a
    /// reload can change *which* paths are watched), and report that the
    /// caller must re-run the pass. Pure R-file edits leave the snapshot
    /// untouched, so R-only iterations skip the reload entirely.
    fn poll_static_inputs(&mut self) -> bool {
        let current = snapshot_static_inputs(
            &config_candidates(&self.search_start),
            self.cfg.baseline.as_deref(),
            &self.cfg.typeshed,
        );
        if current == self.static_stamps {
            return false;
        }
        self.reload();
        self.sync_static_stamps();
        true
    }

    /// Compare the package-metadata dependencies against the snapshot.
    /// `rederive` is set by the caller when an input to the derivation
    /// may have moved — the file set changed or the discovery config
    /// reloaded (the CLI roots are fixed for the session); every other
    /// poll stats the cached [`WatchState::meta_paths`] members only,
    /// skipping the HashSet and join work of a fresh derivation. On any
    /// difference — an mtime edit, a creation/deletion (`None` swapping
    /// with a stamp), or the dependency set itself moving — install the
    /// just-captured snapshot and report that a pass is needed. The
    /// installed values are the pre-check ones, so a metadata edit that
    /// lands while the re-check runs still differs on the next poll
    /// instead of being swallowed by re-reading after the check. Each
    /// poll derives at most once: the `rederive` path derives set and
    /// snapshot together ([`WatchState::derive_meta_state`]), and a
    /// difference installs that fresh pair directly — the set was
    /// derived from the disk state of THIS poll, so any absorbing
    /// boundary move is already reflected and deriving again at the
    /// tail would rebuild an identical set. Only the cached path
    /// re-derives after a difference: a cached set may have missed a
    /// boundary move (a boundary DESCRIPTION created or deleted — see
    /// [`package_meta_paths`]), so the next poll must watch the
    /// boundary as it now stands. That re-derivation does NOT refresh
    /// the stamps: if the boundary did move, the next poll sees the set
    /// difference against the pre-check snapshot and settles with one
    /// extra pass.
    fn poll_meta_inputs(
        &mut self,
        search_roots: &[PathBuf],
        discovered: &[PathBuf],
        rederive: bool,
    ) -> bool {
        if rederive {
            // One derivation per poll: install the fresh set either
            // way (the inputs that produced the cached one moved), and
            // the fresh snapshot only on a difference.
            let (paths, current) = self.derive_meta_state(search_roots, discovered);
            self.meta_paths = paths;
            if current == self.meta_stamps {
                return false;
            }
            self.meta_stamps = current;
            return true;
        }
        let current = snapshot_meta_paths(&self.meta_paths);
        if current == self.meta_stamps {
            return false;
        }
        self.meta_stamps = current;
        self.meta_paths = package_meta_paths(search_roots, discovered, self.config_root());
        true
    }

    /// Re-derive every reloadable input from disk: re-discover and
    /// re-merge the config, rebuild the filter, reload the baseline
    /// and the stubs. Failures keep the last-good inputs with a
    /// warn-once episode (never a per-poll spam, never a dead
    /// session); recovery prints one note when the file parses again.
    /// A machine-readable `output_format` is also a failure here: the
    /// startup path rejects non-human formats for watch mode, so a
    /// mid-watch switch to one keeps the last-good human format
    /// instead of interleaving JSON with the loop's screen clears.
    fn reload(&mut self) {
        match config::Config::discover(&self.search_start) {
            Ok(found) => {
                let (root, base) = match found {
                    Some((path, cfg)) => (path.parent().map(PathBuf::from), cfg),
                    None => (None, config::Config::default()),
                };
                let cfg = base.merge_cli(self.overrides.clone());
                match ry_checker::format::OutputFormat::parse(&cfg.output_format) {
                    Some(format) if !format.is_human() => self.warn_config_broken(&format!(
                        "--watch requires the full or concise output format, not `{}`",
                        cfg.output_format
                    )),
                    Some(format) => {
                        if self.config_broken {
                            self.config_broken = false;
                            eprintln!("ry: ry.toml parses again; reloaded configuration");
                        }
                        self.config_root = root;
                        self.cfg = cfg;
                        self.format = format;
                        self.filter = ry_checker::filter_from_config(&self.cfg);
                        self.user_stubs = load_user_stubs(&self.cfg.typeshed);
                        self.reload_baseline();
                    }
                    None => self.warn_config_broken(&format!(
                        "unknown --output-format `{}`; expected one of: full, concise, json, github, gitlab, junit",
                        cfg.output_format
                    )),
                }
            }
            Err(error) => self.warn_config_broken(&error.to_string()),
        }
    }

    /// Reload the effective baseline after a config reload. A broken
    /// baseline keeps the last-good entries with a warn-once episode.
    /// A config-level baseline whose file is gone is not "broken":
    /// like a fresh run (which warns and continues without it), the
    /// reload settles to no baseline with the same one-shot warning —
    /// keeping stale acceptances would contradict the fresh-run
    /// contract the method's `None` arm already honors for a removed
    /// key. Only a `--baseline` file that vanishes keeps the previous
    /// entries, since startup treats that flag as load-bearing enough
    /// to abort on.
    fn reload_baseline(&mut self) {
        match self.cfg.baseline.as_deref() {
            Some(path) => match config::load_baseline(path) {
                Ok(value) => {
                    if self.baseline_broken {
                        self.baseline_broken = false;
                        eprintln!("ry: baseline {} loads again; reloaded", path.display());
                    }
                    self.baseline = Some(value);
                }
                Err(error) => {
                    // A config-level baseline whose file is gone settles
                    // to no baseline (see the method docs). `path.exists`
                    // is the missing test rather than downcasting the
                    // miette report: `load_baseline` wraps the I/O error
                    // in a message report, so the chain carries no
                    // `std::io::Error` to match on.
                    if !path.exists() && !self.baseline_from_cli {
                        if self.baseline.take().is_some() {
                            eprintln!("ry: warning: {error}");
                        }
                        self.baseline_broken = false;
                    } else if !self.baseline_broken {
                        self.baseline_broken = true;
                        if self.baseline_from_cli {
                            eprintln!("ry: warning: {error}; keeping the previous baseline");
                        } else {
                            eprintln!("ry: warning: {error}");
                        }
                    }
                }
            },
            None => {
                self.baseline = None;
                self.baseline_broken = false;
            }
        }
    }

    fn warn_config_broken(&mut self, error: &str) {
        if !self.config_broken {
            self.config_broken = true;
            eprintln!("ry: warning: {error}; keeping the last good configuration");
        }
    }
}

/// Candidate `ry.toml` paths for the upward discovery walk from
/// `search_start` (file roots start at their parent). Watched even
/// when absent so a newly created config triggers a reload, and wide
/// enough that a nearer config appearing mid-watch wins on the next
/// re-discovery, exactly as a fresh run would resolve it.
fn config_candidates(search_start: &std::path::Path) -> Vec<PathBuf> {
    // Anchor EXACTLY like `Config::discover` — `..` components kept —
    // because discovery's upward walk (`Path::parent`) steps over dots
    // rather than folding them: from `<cwd>/../project` it probes
    // `<cwd>/..` and then `<cwd>` itself before climbing past both,
    // directories the folded start `<parent-of-cwd>/project` never
    // visits. Watching a folded chain could omit the very `ry.toml`
    // discovery selects, leaving the active config unwatched.
    let abs = discover_start(search_start);
    let mut dir: &std::path::Path = if abs.is_file() {
        abs.parent().unwrap_or(std::path::Path::new("."))
    } else {
        abs.as_path()
    };
    let mut candidates = Vec::new();
    loop {
        candidates.push(dir.join(config::CONFIG_FILENAME));
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    candidates
}

/// Resolve `path` against the process working directory, keeping every
/// lexical component as given (`..` stays `..`). This is `ry_config`'s
/// `Config::discover` anchor verbatim; the config-candidate walk must
/// start from it — not from the folded [`absolutize`] form — so
/// discovery and `poll_static_inputs` step over the same directories
/// in the same order.
fn discover_start(path: &std::path::Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// Resolve `path` against the process working directory and normalize
/// it lexically (drop `.` components, fold `..` against the stack)
/// without touching the filesystem — watched metadata candidates may
/// not exist yet, so `canonicalize` is not an option. Used for the
/// package-metadata chains, NOT for the config candidates (see
/// [`discover_start`]): there the walk must mirror discovery's
/// dot-preserving steps exactly. Folding keeps a relative CLI input
/// (`ry check pkg/R/use.R` from a parent directory) walking the same
/// ancestor chain an absolute one would — the raw walk would otherwise
/// fall out of the path at the empty component
/// `Path::new("pkg").parent()` yields and terminate at the working
/// directory.
///
/// Known limitation, accepted: folding is lexical, so a `..` that
/// crosses a symlinked component (`a/symlink/../pkg/DESCRIPTION`)
/// yields a path the OS never resolves to that file — the kernel
/// resolves `..` against the link TARGET's parent, not against `a`,
/// and the one-shot walks (`Path::ancestors` in
/// `enclosing_package_root`, `Config::discover`) keep the components
/// as given and therefore read through the link. On such a tree the
/// snapshot may watch a location the check pass never reads; the
/// residual risk is a missed auto-refresh (an edit to the file the OS
/// actually resolves changes no watched stamp), never a spurious pass
/// and never a wrong result — the pass that does run reads the same
/// files a fresh check would, and the next unrelated edit or a
/// restart picks the change up. Canonicalizing the deepest existing
/// ancestor instead would stat up the tree on every derivation and
/// churn snapshot entries as creations make ever-deeper prefixes
/// canonicalizable, so the lexical form stands.
fn absolutize(path: &std::path::Path) -> PathBuf {
    let joined = discover_start(path);
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// DESCRIPTION/NAMESPACE paths that affect resolution of the watched
/// roots and the files discovery actually returns. Inputs are resolved
/// to absolute paths first (see [`absolutize`]); each chain then walks
/// from the root itself (a file root's parent, a discovered file's
/// parent) up to the first ABSORBING ancestor and contributes its
/// DESCRIPTION/NAMESPACE candidates, watched even when absent, so
/// turning a folder into a package mid-watch (creating DESCRIPTION),
/// adding a NAMESPACE to a package that had none, or deleting either
/// triggers a re-check.
///
/// An ancestor absorbs a chain when one of:
/// * it is already covered by an earlier chain, whose own walk
///   continued as far up as it needed to;
/// * it holds an existing DESCRIPTION — a package boundary. A fresh
///   one-shot groups every file by its NEAREST existing DESCRIPTION
///   ancestor (`ry_workspace::enclosing_package_root` stops there), so
///   nothing above that boundary can change how any file below it
///   resolves;
/// * it is the config root — the directory holding the active
///   `ry.toml`, which the config-candidate walk already anchors and
///   which is the fallback resolution root for files outside any
///   package.
///
/// Absorbing ancestors are included before the walk stops (their own
/// metadata is a resolution input); with no absorber anywhere the chain
/// runs to the filesystem top, exactly as far as the one-shot grouping
/// walk itself would. The accepted trade applies to the CONFIG ROOT
/// absorber only: a package root created above it is not watched even
/// though a fresh check would honor it (the grouping walk climbs past
/// the config root) — the price of keeping unrelated ancestors (/,
/// /tmp, $HOME) out of the set. Above a DESCRIPTION boundary the stop
/// loses nothing: the nearest-boundary rule (`enclosing_package_root`
/// stops at the first existing DESCRIPTION) hides a higher root from a
/// fresh check too. Deleting an absorbing DESCRIPTION IS watched
/// (it is a candidate in the set), and the next poll re-derives the
/// set, so a moved boundary self-heals.
///
/// The per-file chains are what cover packages nested *below* a
/// watched root: a fresh one-shot resolves each file against its
/// nearest DESCRIPTION ancestor, so every directory on that walk is a
/// resolution input, including directories that only *could* become
/// the package root (a nearer DESCRIPTION created there changes the
/// file's grouping without touching any source byte).
///
/// The set stays bounded by relevant files and their ancestors up to
/// the first absorbing boundary: no recursive scan happens here, the
/// discovered paths come from the loop's existing bounded discovery,
/// and directories with no discovered file below them (a sibling
/// package with no R sources) never enter the set. The result is
/// sorted and deduplicated, so the stamp snapshots derived from it are
/// order-stable.
fn package_meta_paths(
    search_roots: &[PathBuf],
    discovered: &[PathBuf],
    config_anchor: Option<&std::path::Path>,
) -> Vec<PathBuf> {
    let anchor = config_anchor.map(absolutize);
    let mut paths = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    // One collector for both halves of the derivation. `seen` and
    // `paths` are shared state of the same set, so both live in the
    // closure; a `false` return means the directory was already covered
    // and the caller's walk stops. `contains` before `insert` keeps the
    // already-covered common case allocation-free.
    let mut push_dir = |dir: &std::path::Path| -> bool {
        if seen.contains(dir) {
            return false;
        }
        seen.insert(dir.to_path_buf());
        paths.push(dir.join("DESCRIPTION"));
        paths.push(dir.join("NAMESPACE"));
        true
    };
    let mut chains: Vec<&std::path::Path> = search_roots
        .iter()
        .map(|root| {
            if root.is_file() {
                root.parent().unwrap_or(std::path::Path::new("."))
            } else {
                root.as_path()
            }
        })
        .collect();
    chains.extend(discovered.iter().filter_map(|file| file.parent()));
    for start in chains {
        let mut dir: PathBuf = absolutize(start);
        loop {
            if !push_dir(&dir) {
                break; // already covered by an earlier chain
            }
            let absorbed = dir.join("DESCRIPTION").is_file() || Some(&dir) == anchor.as_ref();
            if absorbed {
                break;
            }
            match dir.parent() {
                Some(parent) => dir = parent.to_path_buf(),
                None => break,
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// (path, mtime) over the package-metadata dependencies. Missing paths
/// contribute `None` so creation and deletion register. Order-stable by
/// construction: [`package_meta_paths`] returns a sorted, deduplicated
/// set, so repeated derivations produce the same snapshot order no
/// matter what order `discovered` arrived in.
fn snapshot_meta_paths(meta_paths: &[PathBuf]) -> Vec<(PathBuf, Option<SystemTime>)> {
    meta_paths
        .iter()
        .map(|path| {
            (
                path.clone(),
                std::fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok(),
            )
        })
        .collect()
}

/// (path, mtime) over the config-anchored inputs: the config
/// candidates, the effective baseline file, and every stub file the
/// loader would read (flat plus one nesting level, mirroring
/// `discover_stub_files`). Missing paths contribute `None` so creation
/// and deletion register. Sorted for a stable whole-snapshot
/// comparison.
fn snapshot_static_inputs(
    config_paths: &[PathBuf],
    baseline: Option<&std::path::Path>,
    stub_dirs: &[PathBuf],
) -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut snapshot: Vec<(PathBuf, Option<SystemTime>)> = Vec::new();
    snapshot.extend(config_paths.iter().map(|path| {
        (
            path.clone(),
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok(),
        )
    }));
    if let Some(path) = baseline {
        snapshot.push((
            path.to_path_buf(),
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok(),
        ));
    }
    for dir in stub_dirs {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // One nesting level, like the stub loader.
                        if let Ok(nested) = std::fs::read_dir(&path) {
                            for entry in nested.flatten() {
                                let path = entry.path();
                                if !path.is_dir() {
                                    snapshot.push((
                                        path.clone(),
                                        std::fs::metadata(&path)
                                            .and_then(|meta| meta.modified())
                                            .ok(),
                                    ));
                                }
                            }
                        }
                    } else {
                        snapshot.push((
                            path.clone(),
                            std::fs::metadata(&path)
                                .and_then(|meta| meta.modified())
                                .ok(),
                        ));
                    }
                }
            }
            // A missing stub dir still contributes its own entry so
            // creating it later triggers a reload.
            Err(_) => snapshot.push((
                dir.clone(),
                std::fs::metadata(dir).and_then(|meta| meta.modified()).ok(),
            )),
        }
    }
    snapshot.sort();
    snapshot
}

/// Result of a single check pass: the diagnostics, file count, and
/// parse error count. Used by both one-shot and watch mode to print
/// results and compute the exit code.
pub(crate) struct CheckResult {
    diagnostics: Vec<ry_checker::Diagnostic>,
    file_count: usize,
    parse_errors: usize,
    /// Serialized R data files (`.rda`/`.rdata`) that exceeded the byte cap
    /// and were reduced to a file-stem binding, so unbound-variable (RY010)
    /// analysis for their scope is less precise than usual. Each entry is a
    /// human-readable `path (reason)` string. Surfaced in the summary line
    /// and `--statistics` rather than as a diagnostic so the JSON/diagnostic
    /// stream (consumed by the ecosystem harness) stays stable.
    degraded: Vec<String>,
    /// Directories that could not be read while discovering the input
    /// set. Like parse errors, they fail the run's exit code: a
    /// partially undiscoverable input must not look like a clean check
    /// (#485).
    discovery_errors: usize,
}

/// Whether parser recovery indicates that a file is probably not R source.
///
/// Two guards, so foreign files whose syntax produces many recoverable R
/// expressions are still caught while ordinary R files with a few syntax
/// errors pass: more parse errors than statements, or at least 5 errors
/// making up 15% of statements.
fn is_probably_not_r_source(file: &ry_core::SourceFile) -> bool {
    let parse_errors = file.parse_errors.len();
    let statements = file.stmts.len();

    parse_errors > statements || (parse_errors >= 5 && parse_errors * 100 >= 15 * statements.max(1))
}

impl CheckResult {
    fn print_summary(&self, format: ry_checker::format::OutputFormat, statistics: bool) {
        // Suppress the human summary line for machine-readable formats
        // so it can't corrupt JSON/Github/Gitlab/Junit output (it goes
        // to stderr, but consumers that merge stderr would see it).
        if !format.is_human() && !statistics {
            return;
        }
        // --statistics: per-rule counts (ruff's --statistics). Printed
        // to stderr (with the summary) so it never corrupts the stdout
        // diagnostic stream. Sorted by count descending.
        if statistics {
            let mut counts: std::collections::BTreeMap<&str, (usize, ry_checker::Severity)> =
                std::collections::BTreeMap::new();
            for d in &self.diagnostics {
                counts
                    .entry(d.code)
                    .and_modify(|(c, _)| *c += 1)
                    .or_insert((1, d.severity));
            }
            let mut rows: Vec<_> = counts.into_iter().collect();
            rows.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
            eprintln!("ry: statistics ({} unique rule(s))", rows.len());
            for (code, (n, sev)) in rows {
                eprintln!("  {code:<6} {n:>4}  {sev}");
            }
            eprintln!(
                "ry: checked {} file(s), {} diagnostic(s)",
                self.file_count,
                self.diagnostics.len()
            );
            self.print_degraded();
            return;
        }
        let (errors, warnings) = self.counts();
        eprintln!(
            "ry: checked {} file(s), {} error(s), {} warning(s)",
            self.file_count, errors, warnings
        );
        self.print_degraded();
    }

    /// Error and warning counts, shared by the summary line and the exit
    /// code so the two can never disagree.
    fn counts(&self) -> (usize, usize) {
        let errors = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == ry_checker::Severity::Error)
            .count();
        let warnings = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == ry_checker::Severity::Warning)
            .count();
        (errors, warnings)
    }

    /// Surface scopes whose RY010 (unbound-variable) precision dropped
    /// because a serialized data file exceeded the byte cap and was reduced
    /// to a file-stem binding. Printed to stderr (never the stdout
    /// diagnostic stream) so it is visible in both the human summary and
    /// `--statistics` without disturbing machine-readable output.
    fn print_degraded(&self) {
        if self.degraded.is_empty() {
            return;
        }
        eprintln!(
            "ry: {} degraded scope(s) — serialized data file(s) over the byte cap fell back to file stems; RY010 precision reduced:",
            self.degraded.len()
        );
        for note in &self.degraded {
            eprintln!("  - {note}");
        }
        eprintln!("ry: raise `max-serialized-bytes` in ry.toml to enumerate them precisely");
    }

    fn exit_code(&self, cfg: &config::Config) -> ExitCode {
        let (errors, warnings) = self.counts();
        let failed = errors > 0
            || self.parse_errors > 0
            || self.discovery_errors > 0
            || (cfg.error_on_warning && warnings > 0);
        if cfg.exit_zero || !failed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

/// The resolved inputs of one check pass: the config-derived settings
/// the file set is checked under. The file set itself stays a parameter
/// of `run_check_once` because watch iterations change it.
pub(crate) struct CheckContext<'a> {
    filter: &'a ry_checker::SeverityFilter,
    format: ry_checker::format::OutputFormat,
    resolution_config: &'a config::Config,
    user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    color: bool,
    baseline: Option<&'a config::Baseline>,
    repo_root: Option<&'a std::path::Path>,
    min_confidence: ry_checker::Confidence,
}

/// check's parse-failure policy: report every unreadable or unparseable
/// file on stderr and keep going. Failed files drop out of the pass and
/// count as parse errors, which fail the run's exit code.
fn report_check_parse_failure(
    path: &std::path::Path,
    error: &pipeline::ParseError,
) -> pipeline::FailureAction {
    match error {
        pipeline::ParseError::Read(error) => eprintln!("ry: {}: {}", path.display(), error),
        pipeline::ParseError::Parse(message) => {
            eprintln!("ry: {}: parse error: {}", path.display(), message)
        }
    }
    pipeline::FailureAction::Skip
}

/// Core check logic: parse all files, run the project checker, apply
/// the severity filter, print diagnostics, and return a summary. Used
/// by both one-shot `ry check` and `ry check --watch` iterations.
fn run_check_once(paths: &[PathBuf], ctx: &CheckContext) -> Result<CheckResult> {
    let mut all_diagnostics: Vec<ry_checker::Diagnostic> = Vec::new();
    let mut srcs: HashMap<String, String> = HashMap::new();
    let mut comments: HashMap<String, Vec<ry_core::ast::Comment>> = HashMap::new();
    let mut parse_errors = 0usize;
    let mut file_count = 0usize;
    let mut not_r_diagnostics = Vec::new();
    // Degraded scopes (serialized data over the byte cap), deduplicated and
    // sorted for a stable summary. Keyed on the formatted `path (reason)`.
    let mut degraded: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    // Parallel parsing through the shared thread-local parser pool.
    let parsed = pipeline::parse_files(paths, report_check_parse_failure)
        .expect("check's parse-failure policy never aborts");
    parse_errors += paths.len() - parsed.len();
    let parsed: Vec<Arc<ry_core::SourceFile>> = parsed
        .into_iter()
        .filter(|parsed_file| {
            file_count += 1;
            srcs.insert(parsed_file.path.clone(), parsed_file.source.clone());
            comments.insert(parsed_file.path.clone(), parsed_file.comments.clone());
            if is_probably_not_r_source(parsed_file) {
                not_r_diagnostics.push(ry_checker::Diagnostic::new(
                    ry_checker::Severity::Info,
                    ry_core::Span::new(0, 1, 0, 0),
                    &parsed_file.path,
                    "RY097",
                    "File does not appear to be R source; diagnostics suppressed.",
                ));
                false
            } else {
                true
            }
        })
        .collect();

    // Same per-package grouping as `ry dump-types`; check's fallback
    // resolution root for non-package files is the config root (check has
    // no --project-root flag), else the working directory.
    let groups = pipeline::resolve_groups(
        &parsed,
        ctx.resolution_config,
        &ctx.user_stubs,
        &[ctx.repo_root],
    )?;

    let mut per_file_diagnostics = Vec::new();
    for group in groups {
        per_file_diagnostics.extend(check_project(group.check_input));
        for (path, reason) in group.degraded_scopes {
            degraded.insert(format!("{} ({})", path.display(), reason));
        }
    }

    // Post-processing runs through the shared pipeline
    // (`ry_checker::post_process`) so the CLI and the LSP apply one
    // specified order: inline suppression comments (`# ry: ignore`,
    // `# noqa`, `# ry: ignore-file`), then the severity filter, then
    // [demotion seam — `demote_non_source_paths` on the pipeline, run
    // at the seam below], then baseline subtraction, then the
    // min-confidence threshold. The lexical (comment-based) suppression
    // filter keeps a `#` inside a string literal from being mistaken
    // for a directive.
    let post = ry_checker::PostProcess {
        filter: ctx.filter,
        baseline: ctx.baseline,
        min_confidence: ctx.min_confidence,
        repo_root: ctx.repo_root,
    };
    for (path, diags) in &mut per_file_diagnostics {
        let comments: &[ry_core::ast::Comment] = comments.get(path).map_or(&[], Vec::as_slice);
        let src = srcs.get(path).map_or("", String::as_str);
        *diags = post.pre_demotion(std::mem::take(diags), comments, src);
    }
    // The synthesized not-R diagnostics have no suppression comments to
    // honor, so they enter the pipeline at the severity filter.
    ry_checker::apply_filter_to_diagnostics(&mut not_r_diagnostics, ctx.filter);
    all_diagnostics.append(&mut not_r_diagnostics);
    for (_path, diags) in per_file_diagnostics {
        all_diagnostics.extend(diags);
    }

    // Demotion seam: path-based confidence demotion sits between the
    // severity filter and the baseline, its documented position in the
    // shared order. The stage lives in the shared pipeline, so the LSP
    // demotes non-source paths exactly like the CLI (#492).
    post.demote_non_source_paths(&mut all_diagnostics);
    post.post_demotion(&mut all_diagnostics);

    sort_and_deduplicate_diagnostics(&mut all_diagnostics);

    let rendered = render_diagnostics(&all_diagnostics, ctx.format, &srcs, ctx.color);
    if !rendered.is_empty() {
        // Diagnostics go to stdout (matches ruff/ty): `ry check > log`
        // captures the diagnostics, while the summary line and watch-
        // mode chrome go to stderr.
        print!("{}", rendered);
    }

    Ok(CheckResult {
        diagnostics: all_diagnostics,
        file_count,
        parse_errors,
        degraded: degraded.into_iter().collect(),
        discovery_errors: 0,
    })
}

/// Load package stubs from every `--typeshed` directory, later
/// directories replacing same-named packages from earlier ones. Warnings
/// are surfaced; an unreadable directory keeps the run going.
pub(crate) fn load_user_stubs(
    dirs: &[PathBuf],
) -> Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>> {
    let mut merged = std::collections::BTreeMap::new();
    for dir in dirs {
        match ry_typeshed::load_stub_dir_with_warnings(dir) {
            Ok((stubs, warnings)) => {
                for warning in warnings {
                    eprintln!("ry: warning: {warning}");
                }
                merged.extend(stubs);
            }
            Err(error) => eprintln!("ry: warning: {error}"),
        }
    }
    Arc::new(merged)
}

/// Deterministic diagnostic order (confidence, path, position, code,
/// severity, message), then drop exact duplicates.
pub(crate) fn sort_and_deduplicate_diagnostics(diagnostics: &mut Vec<ry_checker::Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        b.confidence.cmp(&a.confidence).then(
            a.path
                .cmp(&b.path)
                .then(a.span.line.cmp(&b.span.line))
                .then(a.span.col.cmp(&b.span.col))
                .then(a.span.start.cmp(&b.span.start))
                .then(a.span.end.cmp(&b.span.end))
                .then(a.code.cmp(b.code))
                .then(a.severity.as_str().cmp(b.severity.as_str()))
                .then(a.message.cmp(&b.message)),
        )
    });
    diagnostics.dedup_by(|a, b| {
        a.path == b.path
            && a.span == b.span
            && a.code == b.code
            && a.severity == b.severity
            && a.confidence == b.confidence
            && a.message == b.message
    });
}

fn init_tracing(verbose: u8, quiet: u8) {
    let filter = if quiet >= 2 {
        "off"
    } else if quiet == 1 {
        "ry=error"
    } else {
        match verbose {
            0 => "ry=warn",
            1 => "ry=info",
            _ => "ry=trace",
        }
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

/// Returns true if the named argument was explicitly provided on the
/// command line (rather than coming from a clap default value). Used to
/// distinguish "the user passed `--error-on-warning`" from "the field's
/// default of false", which is what lets the `ry.toml` value take
/// effect when the CLI flag is omitted.
fn flag_set(matches: Option<&ArgMatches>, id: &str) -> bool {
    matches.and_then(|m| m.value_source(id)) == Some(ValueSource::CommandLine)
}

fn render_diagnostics(
    diagnostics: &[ry_checker::Diagnostic],
    format: ry_checker::format::OutputFormat,
    srcs: &HashMap<String, String>,
    color: bool,
) -> String {
    if format.is_human() {
        let mut tagged = diagnostics.to_vec();
        for diagnostic in &mut tagged {
            if diagnostic.confidence != ry_checker::Confidence::Medium {
                diagnostic.message = format!(
                    "[{}] {}",
                    diagnostic.confidence.as_str(),
                    diagnostic.message
                );
            }
        }
        return ry_checker::format::render_with_color(&tagged, format, srcs, color);
    }
    ry_checker::format::render_with_color(diagnostics, format, srcs, color)
}

/// Surface a discovery cap hit to the user. A cap hit is never
/// silent: the CLI prints one warning per root when any limit is reached.
pub(crate) fn report_truncation(report: &ry_workspace::TruncationReport, root: &std::path::Path) {
    if !report.any_hit() {
        return;
    }
    if report.max_files_hit {
        eprintln!(
            "ry: warning: file count cap (index.max-files) reached at {}; additional R files were not discovered",
            root.display()
        );
    }
    for (path, size) in &report.oversized_files {
        eprintln!(
            "ry: warning: {} ({} bytes) exceeds the per-file size cap (index.max-file-bytes) and was not discovered",
            path.display(),
            size
        );
    }
    for dir in &report.depth_pruned_dirs {
        eprintln!(
            "ry: warning: directory depth cap (index.max-depth) reached at {}; files below {} were not discovered",
            root.display(),
            dir.display()
        );
    }
}

pub(crate) fn sort_and_deduplicate_paths(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}

/// One discovery pass over the search roots: the discovered file set
/// plus the directories the walk could not read.
struct Scan {
    paths: Vec<PathBuf>,
    read_errors: Vec<(PathBuf, String)>,
}

/// Discover the R files under `search_roots` via the shared bounded
/// discovery module and return them sorted and deduplicated, alongside
/// the directories the walk could not read. The two are kept apart so
/// an unreadable root is never mistaken for an intentionally empty or
/// excluded one (#485). `report` surfaces discovery-cap warnings (the
/// initial scan does; quiet watch polls repeat the same roots and stay
/// silent).
fn rescan(
    search_roots: &[PathBuf],
    config_root: Option<&std::path::Path>,
    cfg: &config::Config,
    report: bool,
    explain: bool,
) -> Scan {
    let mut scan = Scan {
        paths: Vec::new(),
        read_errors: Vec::new(),
    };
    for root in search_roots {
        let result =
            ry_workspace::discover_r_files(root, config_root, cfg, cfg.check_test_fixtures);
        if explain {
            for file in &result.files {
                eprintln!("ry: include {}", file.display());
            }
            for (path, reason) in &result.skipped.entries {
                eprintln!("ry: skip {} ({reason})", path.display());
            }
            if result.skipped.omitted > 0 {
                eprintln!(
                    "ry: {} more skipped paths omitted from explanation",
                    result.skipped.omitted
                );
            }
        }
        scan.paths.extend(result.files);
        scan.read_errors.extend(result.read_errors);
        if report {
            report_truncation(&result.truncated, root);
        }
    }
    sort_and_deduplicate_paths(&mut scan.paths);
    scan.read_errors.sort();
    scan.read_errors.dedup();
    scan
}

/// Exit code for a run that could not walk a requested input: failure,
/// unless `--exit-zero` defuses it — the same policy as parse failures.
fn discovery_exit_code(cfg: &config::Config) -> ExitCode {
    if cfg.exit_zero {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Report directories the discovery walk could not read, in the same
/// one-line shape a read failure gets during parsing (`ry: <path>:
/// <error>`), on stderr so machine-readable stdout stays clean.
fn report_read_errors(read_errors: &[(PathBuf, String)]) {
    for (path, error) in read_errors {
        eprintln!("ry: {}: {error}", path.display());
    }
}

/// Record the current mtime of every path into `stamps`.
fn sync_stamps(paths: &[PathBuf], stamps: &mut HashMap<PathBuf, std::time::SystemTime>) {
    for p in paths {
        if let Ok(meta) = std::fs::metadata(p)
            && let Ok(mtime) = meta.modified()
        {
            stamps.insert(p.clone(), mtime);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::Span;

    fn diag(path: &str, line: usize, col: usize, code: &'static str) -> ry_checker::Diagnostic {
        ry_checker::Diagnostic::new(
            ry_checker::Severity::Warning,
            Span::new(line * 10 + col, line * 10 + col + 1, line, col),
            path,
            code,
            "same message",
        )
    }

    /// `run_check_once` with the tail every test shares: JSON output,
    /// default config and severity filter, no stubs, no color, no
    /// baseline, lowest confidence. Tests supply the paths and the repo
    /// root; anything else they vary themselves.
    fn check_files(paths: &[PathBuf], repo_root: Option<&std::path::Path>) -> CheckResult {
        let filter = ry_checker::SeverityFilter::default();
        let resolution_config = config::Config::default();
        run_check_once(
            paths,
            &CheckContext {
                filter: &filter,
                format: ry_checker::format::OutputFormat::Json,
                resolution_config: &resolution_config,
                user_stubs: Arc::new(std::collections::BTreeMap::new()),
                color: false,
                baseline: None,
                repo_root,
                min_confidence: ry_checker::Confidence::Low,
            },
        )
        .unwrap()
    }

    #[test]
    fn diagnostics_are_sorted_and_exact_duplicates_removed() {
        let mut diagnostics = vec![
            diag("b.R", 1, 0, "RY010"),
            diag("a.R", 2, 0, "RY010"),
            diag("a.R", 2, 0, "RY010"),
            diag("a.R", 1, 0, "RY010"),
        ];

        sort_and_deduplicate_diagnostics(&mut diagnostics);

        let positions: Vec<_> = diagnostics
            .iter()
            .map(|d| (d.path.as_str(), d.span.line, d.span.col, d.code))
            .collect();
        assert_eq!(
            positions,
            vec![
                ("a.R", 1, 0, "RY010"),
                ("a.R", 2, 0, "RY010"),
                ("b.R", 1, 0, "RY010"),
            ]
        );
    }

    /// (#491) Pin the post-processing order `ry check` applies through
    /// the shared pipeline: inline suppression BEFORE baseline
    /// subtraction (a suppressed occurrence must not consume the count
    /// its unsuppressed twin needs) and the min-confidence threshold
    /// AFTER it (below-threshold occurrences still consume budget).
    #[test]
    fn suppression_subtracts_before_the_baseline_and_the_threshold_after() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let file = root.join("diagnostic.R");
        std::fs::write(
            &file,
            "if (c(TRUE, FALSE)) print(1)  # ry: ignore[RY002]\nif (c(TRUE, FALSE)) print(1)\n",
        )
        .unwrap();
        let baseline = config::Baseline {
            version: 1,
            entries: vec![config::BaselineEntry {
                path: "diagnostic.R".to_string(),
                code: "RY002".to_string(),
                message: "`if` condition has length 2; R requires a length-1 condition".to_string(),
                count: 1,
            }],
        };
        let filter = ry_checker::SeverityFilter::default();
        let resolution_config = config::Config::default();
        let ctx = CheckContext {
            filter: &filter,
            format: ry_checker::format::OutputFormat::Json,
            resolution_config: &resolution_config,
            user_stubs: Arc::new(std::collections::BTreeMap::new()),
            color: false,
            baseline: Some(&baseline),
            repo_root: Some(root),
            min_confidence: ry_checker::Confidence::Low,
        };

        // Two identical findings, the first suppressed: the suppression
        // drops it before the baseline subtracts, so the count absorbs
        // the unsuppressed twin and the run is quiet. Subtracting first
        // would consume the count on the suppressed occurrence and
        // leave the twin reported.
        let result = run_check_once(std::slice::from_ref(&file), &ctx).unwrap();
        assert_eq!(result.diagnostics.len(), 0);

        // The same shape without the suppression comment shows the twin
        // (proving the quiet run above is the baseline at work, not the
        // suppression alone).
        std::fs::write(
            &file,
            "if (c(TRUE, FALSE)) print(1)\nif (c(TRUE, FALSE)) print(1)\n",
        )
        .unwrap();
        let result = run_check_once(std::slice::from_ref(&file), &ctx).unwrap();
        assert_eq!(result.diagnostics.len(), 1);

        // Threshold plus baseline, as an outcome pin: with a high
        // min-confidence the medium-confidence RY002 findings are below
        // threshold, so the run is quiet. Real checker output gives
        // every finding on one (path, code, message) key the same
        // code-derived confidence, so this quiet is identical under
        // threshold-first — the leg pins the outcome, not the order;
        // the order pins live in the hand-built `post_process` unit
        // tests.
        let ctx = CheckContext {
            min_confidence: ry_checker::Confidence::High,
            ..ctx
        };
        let result = run_check_once(&[file], &ctx).unwrap();
        assert_eq!(result.diagnostics.len(), 0);
    }

    #[test]
    fn multi_package_scan_keeps_library_bindings_isolated() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        for (root, package) in [(&first, "first"), (&second, "second")] {
            std::fs::create_dir_all(root.join("R")).unwrap();
            std::fs::write(
                root.join("DESCRIPTION"),
                format!("Package: {package}\nVersion: 0.0.0.9000\n"),
            )
            .unwrap();
        }
        std::fs::write(first.join("R/first.R"), "only_in_first <- 1L\n").unwrap();
        std::fs::write(second.join("R/second.R"), "value <- only_in_first\n").unwrap();

        let mut paths =
            ry_workspace::discover_r_files(temp.path(), None, &config::Config::default(), false)
                .files;
        paths.sort();
        let result = check_files(&paths, Some(temp.path()));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010"
                && diagnostic.path.contains("second")
                && diagnostic.message.contains("only_in_first")
        }));
    }

    /// The watch metadata-dependency set must cover every ancestor of
    /// every discovered R file (the nested-package chains below the
    /// watched root), must deduplicate shared ancestors, and must stay
    /// bounded: a directory with no discovered file below it — here a
    /// sibling `empty_pkg` that only *could* become a package — never
    /// enters the set, because nothing discovery returns resolves
    /// through it. The expected paths exist on disk only partially, so
    /// the test also pins that absence does not drop a candidate:
    /// creating any of them mid-watch must register.
    #[test]
    fn package_meta_paths_cover_discovered_ancestors_and_stay_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for dir in ["pkg/R", "other/deep/src", "empty_pkg"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let use_r = root.join("pkg/R/use.R");
        let deep_r = root.join("other/deep/src/deep.R");
        std::fs::write(&use_r, "page <- tags\n").unwrap();
        std::fs::write(&deep_r, "deep <- 1L\n").unwrap();

        let meta = package_meta_paths(&[root.to_path_buf()], &[use_r, deep_r], None);

        // Every ancestor of a discovered file up to the watched root is
        // a resolution input (a nearer DESCRIPTION created in any of
        // them changes the file's package grouping without a source
        // edit), and the root chain itself keeps running upward.
        let mut expected_dirs = vec![root.to_path_buf()];
        for dir in ["pkg/R", "pkg", "other/deep/src", "other/deep", "other"] {
            expected_dirs.push(root.join(dir));
        }
        let mut dir = root.parent();
        while let Some(parent) = dir {
            expected_dirs.push(parent.to_path_buf());
            dir = parent.parent();
        }
        for dir in &expected_dirs {
            for name in ["DESCRIPTION", "NAMESPACE"] {
                assert!(
                    meta.contains(&dir.join(name)),
                    "missing {name} for {}",
                    dir.display()
                );
            }
        }
        // Boundedness: nothing below `empty_pkg` and no duplicates —
        // each unique directory contributes exactly its two candidates.
        assert!(
            !meta
                .iter()
                .any(|path| path.starts_with(root.join("empty_pkg"))),
            "a directory with no discovered R file below it must not be watched: {meta:?}"
        );
        let mut sorted = meta.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), meta.len(), "duplicate paths: {meta:?}");
        assert_eq!(meta.len(), 2 * expected_dirs.len(), "set: {meta:?}");
        // Order stability for whole-snapshot equality: the derivation
        // returns a sorted set regardless of the order `discovered`
        // arrives in (the poll loop's comparison is Vec equality).
        assert!(
            meta.windows(2).all(|pair| pair[0] < pair[1]),
            "meta paths must come out sorted: {meta:?}"
        );
    }

    /// An explicit-file root must keep the same ancestor coverage as
    /// watching its directory: a fresh one-shot resolves the file
    /// against its nearest DESCRIPTION ancestor, so the file's parent
    /// and every package boundary above it stay watchable even though
    /// the root itself is not a directory.
    #[test]
    fn package_meta_paths_cover_explicit_file_root_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("pkg/R")).unwrap();
        let use_r = root.join("pkg/R/use.R");
        std::fs::write(&use_r, "page <- tags\n").unwrap();

        let meta = package_meta_paths(
            std::slice::from_ref(&use_r),
            std::slice::from_ref(&use_r),
            None,
        );

        for dir in [root.join("pkg/R"), root.join("pkg"), root.to_path_buf()] {
            for name in ["DESCRIPTION", "NAMESPACE"] {
                assert!(
                    meta.contains(&dir.join(name)),
                    "missing {name} for {}",
                    dir.display()
                );
            }
        }
    }

    /// A root chain must stop at the first ancestor that absorbs it. An
    /// existing DESCRIPTION far above the watched root is a package
    /// boundary: a fresh one-shot's grouping walk
    /// (`enclosing_package_root`) stops at the first existing DESCRIPTION,
    /// so nothing above it can change how the watched files resolve, and
    /// the watch set must not keep candidates above it either.
    #[test]
    fn root_chain_stops_at_first_existing_description_ancestor() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        std::fs::create_dir_all(base.join("ladder/inner")).unwrap();
        std::fs::write(
            base.join("ladder/DESCRIPTION"),
            "Package: absorber\nVersion: 0.0.0.9000\n",
        )
        .unwrap();
        let root = base.join("ladder/inner");
        let use_r = root.join("use.R");
        std::fs::write(&use_r, "x <- 1L\n").unwrap();

        let meta = package_meta_paths(std::slice::from_ref(&root), &[use_r], None);

        // The root and the absorbing boundary are watched (inclusive).
        for dir in [&root, &base.join("ladder")] {
            for name in ["DESCRIPTION", "NAMESPACE"] {
                assert!(
                    meta.contains(&dir.join(name)),
                    "missing {name} for {}: {meta:?}",
                    dir.display()
                );
            }
        }
        // Nothing above the absorbing ancestor: the boundary's own
        // DESCRIPTION fixed the grouping for everything below it.
        for name in ["DESCRIPTION", "NAMESPACE"] {
            assert!(
                !meta.contains(&base.join(name)),
                "candidate above the absorbing DESCRIPTION must not be watched: {meta:?}"
            );
        }
    }

    /// The config root (the directory holding the active `ry.toml`)
    /// absorbs a root chain: it is the anchor config discovery already
    /// watches and the fallback resolution root, so the chain stops
    /// there instead of climbing to the filesystem top.
    #[test]
    fn root_chain_stops_at_the_config_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        std::fs::create_dir_all(base.join("proj/R")).unwrap();
        std::fs::write(base.join("ry.toml"), "").unwrap();
        let use_r = base.join("proj/R/use.R");
        std::fs::write(&use_r, "x <- 1L\n").unwrap();

        let meta = package_meta_paths(&[base.join("proj")], &[use_r], Some(base));

        // proj, proj/R, and the anchor itself — nothing more.
        let mut expected = Vec::new();
        for dir in [base.join("proj/R"), base.join("proj"), base.to_path_buf()] {
            expected.push(dir.join("DESCRIPTION"));
            expected.push(dir.join("NAMESPACE"));
        }
        expected.sort();
        assert_eq!(meta, expected, "the config anchor must bound the chain");
    }

    /// Relative inputs must walk the same ancestor chains as absolute
    /// ones: `Path::new("pkg").parent()` is `Some("")`, whose
    /// `join("DESCRIPTION")` is the bare relative candidate — a second,
    /// logically identical watch entry next to `pkg/DESCRIPTION` that
    /// also terminates the walk at the working directory instead of
    /// rising past it. The walk resolves inputs against the process
    /// working directory, so every candidate is absolute and each file
    /// is watched exactly once.
    #[test]
    fn relative_roots_walk_absolute_ancestors() {
        let meta = package_meta_paths(
            &[PathBuf::from("pkg/R")],
            &[PathBuf::from("pkg/R/use.R")],
            None,
        );
        let cwd = std::env::current_dir().unwrap();
        assert!(
            meta.iter().all(|p| p.is_absolute()),
            "relative inputs must not leak relative candidates: {meta:?}"
        );
        assert!(
            meta.contains(&cwd.join("pkg/R/DESCRIPTION")),
            "the input's own chain must survive absolutization: {meta:?}"
        );
        assert!(
            meta.contains(&cwd.join("pkg/DESCRIPTION")),
            "the parent chain must survive absolutization: {meta:?}"
        );
        assert!(
            !meta.contains(&PathBuf::from("DESCRIPTION")),
            "the empty path component must not contribute a bare candidate: {meta:?}"
        );
    }

    /// Discovery anchors its upward walk by joining the working
    /// directory WITHOUT folding `..` (`ry_config`'s `Config::discover`
    /// keeps the components and lets `Path::parent` step over them), so
    /// from `<base>/sub/../project` it probes `<base>/project` (through
    /// the dots), then `<base>` (as `sub/..`), then `<base>/sub` itself
    /// — directories a lexically folded start would skip. If discovery
    /// can select a config the candidate walk omits, watch mode never
    /// notices edits to the ACTIVE `ry.toml`. Pin both walks to the
    /// same sequence: for dotted directory and dotted file inputs, the
    /// first existing candidate must be exactly the file discovery
    /// returns.
    #[test]
    fn config_candidates_mirror_discovery_for_dotdot_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::create_dir_all(base.join("project/R")).unwrap();
        let use_r = base.join("project/R/use.R");
        std::fs::write(&use_r, "x <- 1L\n").unwrap();
        // The only config on the chain, placed in `sub`: the dotted walk
        // reaches it after `project` and `base`, while the folded start
        // `<base>/project` never visits `sub` at all. Discovery stops
        // there, so nothing above the tempdir can influence the pick.
        std::fs::write(base.join("sub/ry.toml"), "exit-zero = true\n").unwrap();

        let dotted_dir = base.join("sub/../project");
        let dotted_file = base.join("sub/../project/R/use.R");
        for start in [&dotted_dir, &dotted_file] {
            let found = config::Config::discover(start)
                .unwrap()
                .expect("discovery must find the config placed in sub")
                .0;
            let expected = base.join("sub").join(config::CONFIG_FILENAME);
            assert_eq!(
                found, expected,
                "the fixture's only config is in sub; discovery must pick it for {start:?}"
            );
            let candidates = config_candidates(start);
            assert!(
                candidates.contains(&found),
                "the candidate walk must watch the config discovery selects for {start:?}: {candidates:?}"
            );
            let first_hit = candidates.iter().find(|path| path.is_file());
            assert_eq!(
                first_hit,
                Some(&found),
                "the candidate walk must agree with discovery's selection for {start:?}: {candidates:?}"
            );
        }
    }

    #[test]
    fn collection_skips_rcheck_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("example.Rcheck/R")).unwrap();
        let source = root.join("source.R");
        std::fs::write(&source, "source_missing\n").unwrap();
        std::fs::write(root.join("example.Rcheck/R/copied.R"), "copied_missing\n").unwrap();

        let paths =
            ry_workspace::discover_r_files(root, None, &config::Config::default(), false).files;

        assert_eq!(paths, vec![source.clone()]);

        let result = check_files(&paths, Some(root));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY010")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.path == source.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn package_scan_models_testthat_helpers_dependencies_and_interactive_depends() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("DESCRIPTION"),
            "Package: example\nDepends: survival\nSuggests: mirai\n",
        )
        .unwrap();
        for directory in ["R", "tests/testthat", "data-raw"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        std::fs::write(
            root.join("R/package.R"),
            "internal <- function() 1L\ncount.example <- function(x, ...) x\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tests/testthat/helpers-values.R"),
            "library(purrr)\nlibrary(dplyr)\nhelper_value <- 1L\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tests/testthat/test-package.R"),
            "internal()\nhelper_value\nmap\ndaemons\ndata <- unknown_source()\ndata %>% count(column)\n",
        )
        .unwrap();
        std::fs::write(root.join("data-raw/build.R"), "Surv\n").unwrap();

        let mut paths =
            ry_workspace::discover_r_files(root, None, &config::Config::default(), false).files;
        paths.sort();
        let result = check_files(&paths, Some(root));
        let unresolved: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY010")
            .collect();
        let names: Vec<_> = unresolved.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(names.len(), 1, "unexpected unbound names: {unresolved:?}");
        assert!(names.iter().any(|m| m.contains("Surv")));
        assert!(
            names.iter().all(|m| !m.contains("daemons")),
            "Suggests must be attached in test contexts: {unresolved:?}"
        );
    }

    #[test]
    fn package_scan_models_tinytest_package_namespace_and_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("DESCRIPTION"),
            "Package: example\nDepends: survival\nSuggests: mirai\n",
        )
        .unwrap();
        for directory in ["R", "inst/tinytest"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        std::fs::write(root.join("R/package.R"), "internal <- function() 1L\n").unwrap();
        std::fs::write(
            root.join("inst/tinytest/test-package.R"),
            "expect_equal(internal(), 1L)\nSurv\ndaemons\n",
        )
        .unwrap();

        let mut paths =
            ry_workspace::discover_r_files(root, None, &config::Config::default(), false).files;
        paths.sort();
        let result = check_files(&paths, Some(root));
        let unresolved: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY010")
            .collect();
        let names: Vec<_> = unresolved.iter().map(|d| d.message.as_str()).collect();
        assert!(names.is_empty(), "unexpected unbound names: {unresolved:?}");
        assert!(
            names.iter().all(|m| !m.contains("daemons")),
            "Suggests must be attached in test contexts: {unresolved:?}"
        );
    }

    #[test]
    fn majority_invalid_file_yields_only_ry097() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("ratfor.r");
        std::fs::write(&file, "if )\nfor )\nwhile )\nfunction )\n").unwrap();
        let result = check_files(&[file], Some(temp.path()));
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "RY097");
        assert_eq!(result.diagnostics[0].severity, ry_checker::Severity::Info);
    }

    #[test]
    fn markdown_table_file_yields_only_ry097() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("table.R");
        std::fs::write(
            &file,
            "| Function | Description |\n|----------|-------------|\n| `foo` | Does a thing |\n| `bar` | Does another thing |\n| `baz` | Does one more thing |\n",
        )
        .unwrap();

        let result = check_files(&[file], Some(temp.path()));
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "RY097");
    }

    #[test]
    fn ratfor_style_file_yields_only_ry097() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("inddup.r");
        std::fs::write(
            &file,
            "subroutine inddup(x,y,n,rw,frac,dup)\nimplicit double precision(a-h,o-z)\nlogical dup(n)\ndimension x(n), y(n), rw(4)\ndup(1) = .false.\ndo i = 2,n {\n  dup(i) = .false.\n  do j = 1,i-1 {\n    if(dx < xtol & dy < ytol) {\n      dup(i) = .true.\n    }\n  }\n}\ndo k = 1,n {\n  dup(k) = .false.\n}\ndo k = 1,n {\n  dup(k) = .false.\n}\ndo k = 1,n {\n  dup(k) = .false.\n}\ndo k = 1,n {\n  dup(k) = .false.\n}\nreturn\nend\n",
        )
        .unwrap();

        let result = check_files(&[file], Some(temp.path()));
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "RY097");
    }

    #[test]
    fn latin1_source_comment_does_not_skip_checking() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("latin1.R");
        std::fs::write(&file, b"# Caf\xe9\nmissing_name\n").unwrap();

        let result = check_files(&[file], Some(temp.path()));

        assert_eq!(result.file_count, 1);
        assert_eq!(result.parse_errors, 0);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY010")
        );
        // R's parser tolerates invalid bytes inside comments, so the
        // file must not gain an encoding RY000 either (#376).
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY000"),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn latin1_bytes_in_a_string_are_flagged_as_an_encoding_ry000() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("latin1_string.R");
        std::fs::write(&file, b"label <- \"caf\xe9 au lait\"\n").unwrap();

        let result = check_files(&[file], Some(temp.path()));

        // Like recovered-tree files, an encoding-flagged file reports
        // only its RY000.
        let encoding: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY000")
            .collect();
        assert_eq!(encoding.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert!(
            encoding[0].message.contains("not valid UTF-8"),
            "{:?}",
            result.diagnostics
        );
    }

    /// A leading UTF-8 BOM is valid UTF-8, but R's parser rejects the
    /// file with "unexpected input" at 1:1 (#474): `ry check` must flag
    /// it like the non-UTF-8 case (#376) instead of checking clean.
    #[test]
    fn leading_bom_is_flagged_as_an_encoding_ry000() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("bom.R");
        std::fs::write(&file, b"\xef\xbb\xbfx <- 1\nmissing_name\n").unwrap();

        let result = check_files(&[file], Some(temp.path()));

        // Like recovered-tree and non-UTF-8 files, a BOM-flagged file
        // reports only its RY000: the unbound name below it is noise on
        // a file R refuses at 1:1.
        let encoding: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY000")
            .collect();
        assert_eq!(encoding.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert!(
            encoding[0].message.contains("byte order mark"),
            "{:?}",
            result.diagnostics
        );
    }

    /// The adjacent idiom that must stay quiet: the same U+FEFF character
    /// anywhere but the file's first bytes is an ordinary character R's
    /// parser accepts.
    #[test]
    fn bom_character_elsewhere_in_the_file_stays_clean() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("bom_midfile.R");
        std::fs::write(&file, "s <- \"\u{feff}\"\nx <- 1 # \u{feff} comment\n").unwrap();

        let result = check_files(&[file], Some(temp.path()));

        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY000"),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn fifty_statement_r_file_with_three_syntax_errors_does_not_collapse() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("mostly-valid.R");
        let mut source: String = (1..=50).map(|i| format!("x{i} <- {i}\n")).collect();
        source.push_str("if )\nif )\nif )\n");
        std::fs::write(&file, source).unwrap();

        let result = check_files(&[file], Some(temp.path()));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY000")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY097")
        );
    }

    #[test]
    fn four_statement_r_file_with_one_error_does_not_collapse_via_ratio_rule() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("small.R");
        std::fs::write(&file, "a <- 1\nb <- 2\nc <- 3\nd <- 4\nif )\n").unwrap();

        let result = check_files(&[file], Some(temp.path()));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY000")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY097")
        );
    }

    #[test]
    fn check_project_basic() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("test.R", "x <- 1\n").unwrap();
        let input = CheckInput {
            files: vec![("test.R".to_string(), Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: Default::default(),
        };
        let output = check_project(input);
        // A clean file should produce no diagnostics.
        let total: usize = output.iter().map(|(_, d)| d.len()).sum();
        assert_eq!(total, 0, "clean file should have no diagnostics");
    }

    #[test]
    fn check_project_finds_undefined_var() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("test.R", "undefined_var\n").unwrap();
        let input = CheckInput {
            files: vec![("test.R".to_string(), Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: Default::default(),
        };
        let output = check_project(input);
        let total: usize = output.iter().map(|(_, d)| d.len()).sum();
        assert!(total > 0, "undefined variable should produce diagnostics");
    }

    #[test]
    fn check_project_with_workspace_context() {
        // `is_null(x)` narrows `x` away from NULL only when its defining
        // package is attached, so the trailing `x()` is an RY070
        // (calling a non-function) exactly when rlang is absent. Running
        // the same source with and without the workspace context proves
        // check_project actually feeds attached_packages into the checker
        // instead of ignoring it.
        let src = "x <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n";
        let run = |workspace: ry_workspace::WorkspaceContext| -> usize {
            let mut parser = ry_core::RParser::new().unwrap();
            let file = parser.parse("test.R", src).unwrap();
            let output = check_project(CheckInput {
                files: vec![("test.R".to_string(), Arc::new(file))],
                user_stubs: Arc::new(BTreeMap::new()),
                workspace,
            });
            output
                .iter()
                .flat_map(|(_, diags)| diags.iter())
                .filter(|d| d.code == "RY070")
                .count()
        };

        let without = run(Default::default());
        assert!(
            without > 0,
            "without rlang the predicate cannot narrow x; RY070 must fire for x()"
        );

        let mut with = ry_workspace::WorkspaceContext::default();
        with.attached_packages.insert("rlang".to_string());
        assert_eq!(
            run(with),
            0,
            "with rlang attached the predicate narrows x; no RY070 may survive"
        );
    }

    #[test]
    fn check_project_cross_file_resolution() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file_a = parser.parse("a.R", "shared_fn <- function(x) x\n").unwrap();
        let file_b = parser.parse("b.R", "shared_fn(42)\n").unwrap();
        let input = CheckInput {
            files: vec![
                ("a.R".to_string(), Arc::new(file_a)),
                ("b.R".to_string(), Arc::new(file_b)),
            ],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: Default::default(),
        };
        let output = check_project(input);
        // shared_fn is defined in a.R and called in b.R — should resolve.
        let b_diags: usize = output
            .iter()
            .find(|(p, _)| p == "b.R")
            .map(|(_, d)| d.len())
            .unwrap_or(0);
        assert_eq!(b_diags, 0, "cross-file function call should resolve");
    }

    #[test]
    fn check_project_with_scope_capture_returns_records_per_file() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse("a.R", "f <- function(x = 1L) { y <- x\n y }\n")
            .unwrap();
        let records = check_project_with_scope_capture(CheckInput {
            files: vec![("a.R".to_string(), Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: Default::default(),
        });
        assert_eq!(records.len(), 1);
        let (path, file_records) = &records[0];
        assert_eq!(path, "a.R");
        // Exactly the top scope and the one function scope.
        assert_eq!(file_records.len(), 2, "{file_records:?}");
        let function = file_records
            .iter()
            .find(|r| r.kind == ry_checker::ScopeRecordKind::Function)
            .expect("function scope recorded");
        assert_eq!(function.name.as_deref(), Some("f"));
        assert_eq!(function.params.len(), 1);
    }
}
