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
    pub workspace: Option<ry_workspace::WorkspaceContext>,
}

/// Result of a unified diagnostics check.
pub struct CheckOutput {
    /// Per-file diagnostics: (path, Vec<Diagnostic>).
    pub diagnostics: Vec<(String, Vec<ry_checker::Diagnostic>)>,
}

/// Run a one-shot project check with workspace metadata.
///
/// This is the single entry point for diagnostics computation.
/// ry-cli calls this instead of coordinating Project setters.
pub fn check_project(input: CheckInput) -> CheckOutput {
    let mut project = apply_workspace(
        ry_checker::Project::new(),
        input.workspace.as_ref(),
        &input.user_stubs,
    );

    for (path, file) in &input.files {
        project.add_file_arc(path.clone(), Arc::clone(file));
    }

    let diagnostics = project.check();

    CheckOutput { diagnostics }
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
    let mut project = apply_workspace(
        ry_checker::Project::new(),
        input.workspace.as_ref(),
        &input.user_stubs,
    );
    for (path, file) in &input.files {
        project.add_file_arc(path.clone(), Arc::clone(file));
    }
    project.enable_scope_capture();
    project.check();
    project.take_scope_records()
}

/// Install the workspace metadata onto a fresh project. Shared by
/// [`check_project`] and [`check_project_with_scope_capture`] so the two
/// entry points can never drift in what environment they model.
fn apply_workspace(
    mut project: ry_checker::Project,
    workspace: Option<&ry_workspace::WorkspaceContext>,
    user_stubs: &Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
) -> ry_checker::Project {
    let empty_workspace = ry_workspace::WorkspaceContext::default();
    let workspace = workspace.unwrap_or(&empty_workspace);
    project.set_loaded(workspace.attached_packages.clone());
    project.set_bare_loaded(workspace.bare_bindings.clone());
    project.set_user_stubs(Arc::clone(user_stubs));
    project.set_external_bindings(workspace.external_bindings.clone());
    project.set_imported_from(workspace.imported_bindings.clone());
    project.set_external_s3_methods(workspace.s3_methods.clone());
    project.set_load_bindings(workspace.load_bindings.clone());
    project
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
        write_baseline,
        baseline,
        min_confidence,
    } = args;

    // Config discovery is anchored at the first input path (itself for a
    // directory, its parent for a file — `Config::discover` applies that
    // rule) or at the working directory when no paths were given, the
    // same anchor `ry dump-types` uses.
    let search_start = paths
        .first()
        .map(|p| p.as_path())
        .unwrap_or_else(|| std::path::Path::new("."));

    let (config_root, base_cfg) = match pipeline::discover_config(search_start) {
        Ok(found) => found,
        Err(code) => return Ok(code),
    };

    // Forward `None` for scalars the CLI did not set explicitly, so the
    // config file's value wins.
    let m = check_matches;
    let baseline_from_cli = flag_set(m, "baseline");

    let cfg = base_cfg.merge_cli(config::CliOverrides {
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
    });

    let baseline = match cfg.baseline.as_deref() {
        Some(path) => match config::load_baseline(path) {
            Ok(value) => Some(value),
            Err(error) if baseline_from_cli => return Err(error),
            Err(error) => {
                eprintln!("ry: warning: {error}");
                None
            }
        },
        None => None,
    };

    // Re-init tracing with the merged verbosity so a `verbose = 2` in
    // ry.toml takes effect even when the user runs a bare `ry check`.
    // `try_init` is idempotent (the first subscriber wins), so if main
    // already installed one this is a no-op; that's fine because main
    // used the CLI counts which are a superset here.
    init_tracing(cfg.verbose, cfg.quiet);

    let format = ry_checker::format::OutputFormat::parse(&cfg.output_format).ok_or_else(|| {
        miette::miette!(
            "unknown --output-format `{}`; expected one of: full, concise, json, github, gitlab, junit",
            cfg.output_format
        )
    })?;
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
    let mut all_paths = rescan(&search_roots, config_root.as_deref(), &cfg, true);

    if all_paths.is_empty() {
        let roots = search_roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("ry: no .R / .r files found in {roots}");
        return Ok(ExitCode::SUCCESS);
    }

    // The stable inputs of every pass. Only the file set changes across
    // watch iterations, so it stays a parameter of `run_check_once`.
    let ctx = CheckContext {
        filter: &filter,
        format,
        resolution_config: &cfg,
        user_stubs: Arc::clone(&user_stubs),
        color,
        baseline: baseline.as_ref(),
        repo_root: config_root.as_deref(),
        min_confidence: min_confidence.into(),
    };

    let result = run_check_once(&all_paths, &ctx)?;
    if let Some(path) = write_baseline.as_deref() {
        config::write_baseline_file(path, &result.diagnostics, config_root.as_deref())?;
    }
    result.print_summary(format, statistics);

    if !watch {
        return Ok(result.exit_code(&cfg));
    }
    if !format.is_human() {
        eprintln!("ry: --watch requires the full or concise output format");
        return Ok(ExitCode::FAILURE);
    }

    eprintln!(
        "ry: watching {} file(s) for changes (Ctrl+C to stop)...",
        all_paths.len()
    );
    let mut stamps: HashMap<PathBuf, std::time::SystemTime> = HashMap::new();
    sync_stamps(&all_paths, &mut stamps);

    let poll_interval = std::time::Duration::from_millis(500);
    loop {
        std::thread::sleep(poll_interval);

        // Re-scan for new/deleted files via shared bounded discovery.
        // Truncation was already reported on the initial scan, so the
        // poll keeps stderr quiet.
        let current_paths = rescan(&search_roots, config_root.as_deref(), &cfg, false);

        // Check for any file modification or file set change.
        let mut changed = current_paths != all_paths;
        if !changed {
            for p in &current_paths {
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
            all_paths = current_paths;
            // Re-sync stamps for any new files.
            sync_stamps(&all_paths, &mut stamps);
            // Clear screen for a clean view of the new diagnostics.
            // Using ANSI escape sequences rather than `clear` command
            // for portability (no external process spawn).
            eprint!("\x1b[2J\x1b[H");
            let result = run_check_once(&all_paths, &ctx)?;
            result.print_summary(format, statistics);
        }
    }
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
        let failed = errors > 0 || self.parse_errors > 0 || (cfg.error_on_warning && warnings > 0);
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
        let check_output = check_project(group.check_input);
        per_file_diagnostics.extend(check_output.diagnostics);
        for (path, reason) in group.degraded_scopes {
            degraded.insert(format!("{} ({})", path.display(), reason));
        }
    }

    // Apply inline suppression comments (`# ry: ignore`, `# noqa`,
    // `# ry: ignore-file`) before the severity filter so a suppressed
    // error never even reaches the filter pipeline. Use the lexical
    // (comment-based) filter so a `#` inside a string literal is not
    // mistaken for a suppression directive.
    for (path, diags) in &mut per_file_diagnostics {
        if let Some(cs) = comments.get(path) {
            let src = srcs.get(path).map(String::as_str).unwrap_or("");
            *diags = ry_checker::filter_suppressed_with_comments(std::mem::take(diags), cs, src);
        }
    }

    for (_path, diags) in &mut per_file_diagnostics {
        ry_checker::apply_filter_to_diagnostics(diags, ctx.filter);
    }
    ry_checker::apply_filter_to_diagnostics(&mut not_r_diagnostics, ctx.filter);
    all_diagnostics.append(&mut not_r_diagnostics);
    for (_path, diags) in per_file_diagnostics {
        all_diagnostics.extend(diags);
    }

    demote_non_source_paths(&mut all_diagnostics, ctx.repo_root);
    if let Some(baseline) = ctx.baseline {
        config::subtract_baseline(&mut all_diagnostics, baseline, ctx.repo_root);
    }
    all_diagnostics.retain(|diagnostic| diagnostic.confidence >= ctx.min_confidence);

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

/// Demote diagnostics from a package's non-source trees (`tests/`,
/// `data-raw/`, `demo/`, `vignettes/`, `inst/`) one confidence tier:
/// code there is not what CRAN ships or checks first.
pub(crate) fn demote_non_source_paths(
    diagnostics: &mut [ry_checker::Diagnostic],
    repo_root: Option<&std::path::Path>,
) {
    const DEMOTED: [&str; 5] = ["tests", "data-raw", "demo", "vignettes", "inst"];
    for diagnostic in diagnostics {
        let path = std::path::Path::new(&diagnostic.path);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            repo_root
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(path)
        };
        let mut package_root = absolute.parent();
        while let Some(root) = package_root {
            if root.join("DESCRIPTION").is_file() {
                if let Ok(relative) = absolute.strip_prefix(root)
                    && relative.components().any(|component| {
                        component
                            .as_os_str()
                            .to_str()
                            .is_some_and(|name| DEMOTED.contains(&name))
                    })
                {
                    diagnostic.confidence = diagnostic.confidence.demote();
                }
                break;
            }
            package_root = root.parent();
        }
    }
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

/// Discover the R files under `search_roots` via the shared bounded
/// discovery module and return them sorted and deduplicated. `report`
/// surfaces discovery-cap warnings (the initial scan does; quiet watch
/// polls repeat the same roots and stay silent).
fn rescan(
    search_roots: &[PathBuf],
    config_root: Option<&std::path::Path>,
    cfg: &config::Config,
    report: bool,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for root in search_roots {
        let result =
            ry_workspace::discover_r_files(root, config_root, cfg, cfg.check_test_fixtures);
        paths.extend(result.files);
        if report {
            report_truncation(&result.truncated, root);
        }
    }
    sort_and_deduplicate_paths(&mut paths);
    paths
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

    #[test]
    fn package_tests_path_demotes_confidence_one_tier() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir(temp.path().join("tests")).unwrap();
        let path = temp.path().join("tests/test.R");
        let mut diagnostic = diag(path.to_str().unwrap(), 1, 0, "RY030");
        assert_eq!(diagnostic.confidence, ry_checker::Confidence::High);
        demote_non_source_paths(std::slice::from_mut(&mut diagnostic), Some(temp.path()));
        assert_eq!(diagnostic.confidence, ry_checker::Confidence::Medium);
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
            workspace: None,
        };
        let output = check_project(input);
        // A clean file should produce no diagnostics.
        let total: usize = output.diagnostics.iter().map(|(_, d)| d.len()).sum();
        assert_eq!(total, 0, "clean file should have no diagnostics");
    }

    #[test]
    fn check_project_finds_undefined_var() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("test.R", "undefined_var\n").unwrap();
        let input = CheckInput {
            files: vec![("test.R".to_string(), Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: None,
        };
        let output = check_project(input);
        let total: usize = output.diagnostics.iter().map(|(_, d)| d.len()).sum();
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
        let run = |workspace: Option<ry_workspace::WorkspaceContext>| -> usize {
            let mut parser = ry_core::RParser::new().unwrap();
            let file = parser.parse("test.R", src).unwrap();
            let output = check_project(CheckInput {
                files: vec![("test.R".to_string(), Arc::new(file))],
                user_stubs: Arc::new(BTreeMap::new()),
                workspace,
            });
            output
                .diagnostics
                .iter()
                .flat_map(|(_, diags)| diags.iter())
                .filter(|d| d.code == "RY070")
                .count()
        };

        let without = run(None);
        assert!(
            without > 0,
            "without rlang the predicate cannot narrow x; RY070 must fire for x()"
        );

        let mut with = ry_workspace::WorkspaceContext::default();
        with.attached_packages.insert("rlang".to_string());
        assert_eq!(
            run(Some(with)),
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
            workspace: None,
        };
        let output = check_project(input);
        // shared_fn is defined in a.R and called in b.R — should resolve.
        let b_diags: usize = output
            .diagnostics
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
            workspace: None,
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
