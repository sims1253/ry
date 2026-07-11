#![allow(clippy::collapsible_if)]

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::parser::ValueSource;
use clap::{
    ArgMatches, CommandFactory, FromArgMatches, Parser as ClapParser, Subcommand, ValueEnum,
};
use miette::{IntoDiagnostic, Result};

mod config;
mod package_metadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    fn enabled(self, format: ry_checker::format::OutputFormat) -> bool {
        self.enabled_for(
            format,
            std::io::stdout().is_terminal(),
            std::env::var_os("NO_COLOR").is_some(),
        )
    }

    fn enabled_for(
        self,
        format: ry_checker::format::OutputFormat,
        stdout_is_terminal: bool,
        no_color: bool,
    ) -> bool {
        if !matches!(
            format,
            ry_checker::format::OutputFormat::Full | ry_checker::format::OutputFormat::Concise
        ) {
            return false;
        }
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => !no_color && stdout_is_terminal,
        }
    }
}

#[derive(Debug, ClapParser)]
#[command(
    name = "ry",
    version,
    about = "A fast static checker for R",
    long_about = "ry is a static type checker for R, inspired by astral-sh/ty."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Increase verbosity. Use -v for debug, -vv for trace.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// Decrease verbosity. Use -q for quiet, -qq for silent.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    quiet: u8,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Check a project (or files) for type errors.
    Check {
        /// Files or directories to check. Defaults to the current working
        /// directory, mirroring `ty check` semantics.
        paths: Vec<PathBuf>,
        /// Treat the given rule as severity 'error'. Accepts a rule code
        /// (RY040), a rule name (invalid-arithmetic), or 'all'. Repeatable.
        #[arg(long)]
        error: Vec<String>,
        /// Treat the given rule as severity 'warn'. Same syntax as --error.
        #[arg(long)]
        warn: Vec<String>,
        /// Disable the rule entirely. Same syntax as --error.
        #[arg(long)]
        ignore: Vec<String>,
        /// Use exit code 1 if there are any warning-level diagnostics.
        #[arg(long)]
        error_on_warning: bool,
        /// Always use exit code 0, even if there are error-level diagnostics.
        #[arg(long)]
        exit_zero: bool,
        /// Output format. One of: full, concise, json, github, gitlab, junit.
        /// `full` is the default (matches ty); `concise` is available for a
        /// one-line-per-diagnostic view.
        #[arg(long, value_name = "FORMAT", default_value = "full")]
        output_format: String,
        /// Control ANSI color in human-readable output.
        #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
        color: ColorChoice,
        /// Watch for file changes and re-check automatically.
        /// Uses polling (500ms interval). Press Ctrl+C to stop.
        #[arg(short = 'W', long)]
        watch: bool,
        /// Print per-rule diagnostic counts after the run (ruff's
        /// `--statistics`). Useful for corpus research and triage.
        #[arg(long)]
        statistics: bool,
    },
    /// Start the language server. Speaks the Language Server Protocol
    /// (LSP) over stdio, publishing type-check diagnostics for open R
    /// files. Connect to it from any LSP-aware editor (VS Code, Neovim,
    /// Helix, etc.).
    Server,
    /// Display ry's version.
    Version {
        /// Output format for version info.
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: String,
    },
    /// Explain a rule (or all rules). `ry rule` is an alias (matches
    /// ruff's `ruff rule`).
    #[command(visible_alias = "rule")]
    ExplainRule {
        /// Rule code or name. Omit to list all rules.
        rule: Option<String>,
        /// Output format: text or json.
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: String,
    },
    /// Show the embedded typeshed (debug).
    ExplainTypeshed,
    /// Generate shell completions.
    GenerateShellCompletion {
        /// Target shell.
        shell: String,
    },
}

fn main() -> Result<ExitCode> {
    // We parse into the typed `Cli` for ergonomic access to argument
    // values, but we ALSO retain the underlying `ArgMatches` so we can
    // distinguish "the user passed --error-on-warning on the command
    // line" from "the default value of false". That distinction is what
    // lets a `ry.toml` `error-on-warning = true` take effect when the
    // user runs a bare `ry check` (no flags).
    //
    // clap derive's `from_arg_matches` is infallible for our schema
    // (every arg has a default or is optional); the unwrap is safe.
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("clap derive schema is self-consistent");

    // Tracing is initialized inside `run_check` AFTER config discovery
    // so a `verbose = N` in `ry.toml` can take effect. Non-check
    // subcommands do not emit tracing events, so they do not need an
    // earlier init.

    let cmd = match cli.cmd {
        Some(c) => c,
        None => Cmd::Check {
            paths: Vec::new(),
            error: Vec::new(),
            warn: Vec::new(),
            ignore: Vec::new(),
            error_on_warning: false,
            exit_zero: false,
            output_format: "full".to_string(),
            color: ColorChoice::Auto,
            watch: false,
            statistics: false,
        },
    };

    // Subcommand matches are nested under the subcommand's name. We
    // only need them for `check` (to detect explicit CLI overrides of
    // scalar fields that the config file can also set).
    let check_matches = matches.subcommand_matches("check");

    match cmd {
        Cmd::Check {
            paths,
            error,
            warn,
            ignore,
            error_on_warning,
            exit_zero,
            output_format,
            color,
            watch,
            statistics,
        } => run_check(
            paths,
            error,
            warn,
            ignore,
            error_on_warning,
            exit_zero,
            &output_format,
            color,
            cli.verbose,
            cli.quiet,
            check_matches,
            watch,
            statistics,
        ),
        Cmd::Server => {
            // The LSP server reads JSON-RPC from stdin and writes
            // JSON-RPC to stdout. CRITICAL: any tracing or log output
            // on stdout will corrupt the stream. We install a tracing
            // subscriber that writes ONLY to stderr, with a conservative
            // `ry=warn` filter so the server stays quiet by default.
            //
            // `try_init` is idempotent (the first subscriber wins); if a
            // subscriber was already installed earlier in this process,
            // this call is a no-op. We don't rely on that, but it means
            // we don't have to coordinate with `run_check`'s init.
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter("ry=warn")
                .try_init()
                .ok();
            // The LSP server is async (tower-lsp is built on tokio), but
            // `main` is synchronous. We spin up a multi-threaded tokio
            // runtime for the server case only. Other subcommands keep
            // their synchronous behavior and pay no runtime cost.
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| miette::miette!("failed to start tokio runtime: {}", e))?;
            rt.block_on(async { ry_lsp::run().await })
                .map_err(|e| miette::miette!("ry LSP server error: {}", e))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Version { output_format } => {
            print_version(&output_format);
            Ok(ExitCode::SUCCESS)
        }
        Cmd::ExplainRule {
            rule,
            output_format,
        } => run_explain_rule(rule, &output_format),
        Cmd::ExplainTypeshed => run_explain_typeshed(),
        Cmd::GenerateShellCompletion { shell } => run_shell_completion(&shell),
    }
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
            _ => "ry=debug",
        }
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn build_filter(
    error: &[String],
    warn: &[String],
    ignore: &[String],
) -> ry_checker::SeverityFilter {
    let mut f = ry_checker::SeverityFilter::default();
    for e in error {
        f.add_error(e);
    }
    for w in warn {
        f.add_warn(w);
    }
    for i in ignore {
        f.add_ignore(i);
    }
    f
}

/// Returns true if the named argument was explicitly provided on the
/// command line (rather than coming from a clap default value). Used to
/// distinguish "the user passed `--error-on-warning`" from "the field's
/// default of false", which is what lets the `ry.toml` value take
/// effect when the CLI flag is omitted.
fn flag_set(matches: Option<&ArgMatches>, id: &str) -> bool {
    matches.and_then(|m| m.value_source(id)) == Some(ValueSource::CommandLine)
}

/// Compute the path of `file` relative to `root`, as a forward-slash
/// string suitable for matching against `ry.toml` `exclude` patterns.
///
/// Both inputs are first canonicalized so that a relative `ry check
/// ./src` invocation still matches patterns written against the
/// project-relative form (e.g. `src/**`). If canonicalization fails
/// (e.g. a missing path), we fall back to a best-effort strip of the
/// root prefix from the literal path, and finally to the file's full
/// display string, so exclude matching degrades gracefully rather than
/// panicking.
fn relative_path_for_exclude(file: &std::path::Path, root: &std::path::Path) -> String {
    let canon_file = std::fs::canonicalize(file).ok();
    let canon_root = std::fs::canonicalize(root).ok();
    if let (Some(f), Some(r)) = (canon_file, canon_root) {
        if let Ok(rel) = f.strip_prefix(&r) {
            return rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
        }
    }
    // Best-effort fallback: strip the root's literal prefix.
    if let Ok(rel) = file.strip_prefix(root) {
        return rel
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
    }
    file.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[allow(clippy::too_many_arguments)]
fn run_check(
    paths: Vec<PathBuf>,
    error: Vec<String>,
    warn: Vec<String>,
    ignore: Vec<String>,
    error_on_warning: bool,
    exit_zero: bool,
    output_format: &str,
    color: ColorChoice,
    cli_verbose: u8,
    cli_quiet: u8,
    check_matches: Option<&ArgMatches>,
    watch: bool,
    statistics: bool,
) -> Result<ExitCode> {
    // Determine the search start directory for config discovery. If the
    // user passed a path, anchor discovery at the first path's parent
    // (for files) or at the path itself (for directories). With no
    // paths, discovery starts from the current working directory.
    let search_start: PathBuf = paths
        .first()
        .map(|p| {
            if p.is_dir() {
                p.clone()
            } else {
                p.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            }
        })
        .unwrap_or_else(|| PathBuf::from("."));

    // Discover a ry.toml by walking up from the search start. A missing
    // config is not an error; we fall back to `Config::defaults()`. A
    // present-but-malformed config IS an error: surface it and abort so
    // the user notices the typo rather than silently running with
    // defaults.
    let (config_root, base_cfg) = match config::Config::discover(&search_start) {
        Ok(Some((path, cfg))) => {
            tracing::debug!(config = %path.display(), "loaded ry.toml");
            (path.parent().map(|p| p.to_path_buf()), cfg)
        }
        Ok(None) => (None, config::Config::defaults()),
        Err(e) => {
            eprintln!("ry: {}", e);
            return Ok(ExitCode::FAILURE);
        }
    };

    // Determine which scalar CLI flags were explicitly set on the
    // command line. `value_source == CommandLine` distinguishes a
    // user-provided value from the clap default. When the CLI did NOT
    // set a scalar, we forward `None` so the config file's value wins.
    let m = check_matches;
    let cli_error_on_warning = flag_set(m, "error_on_warning").then_some(error_on_warning);
    let cli_exit_zero = flag_set(m, "exit_zero").then_some(exit_zero);
    let cli_output_format = flag_set(m, "output_format").then_some(output_format.to_string());

    let cfg = base_cfg.merge_cli(
        error,
        warn,
        ignore,
        cli_error_on_warning,
        cli_exit_zero,
        cli_output_format,
        cli_verbose,
        cli_quiet,
    );

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
    let filter = build_filter(&cfg.error, &cfg.warn, &cfg.ignore);
    let excludes = config::Excludes::from_config(&cfg);

    // Collect the initial file set.
    let mut all_paths = Vec::new();
    let search_roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths
    };
    for root in &search_roots {
        collect_r_files(root, &mut all_paths);
    }
    sort_and_deduplicate_paths(&mut all_paths);

    // Apply exclude patterns. Patterns match against the path relative
    // to the directory containing the originating `ry.toml`; if no
    // config was found, nothing is excluded. We use forward-slash
    // separators to match the glob crate's expectations.
    if let Some(root) = config_root.as_ref() {
        if !excludes.is_empty() {
            all_paths.retain(|p| {
                let rel = relative_path_for_exclude(p, root);
                if excludes.matches(&rel) {
                    tracing::debug!(path = %p.display(), "excluded by ry.toml");
                    false
                } else {
                    true
                }
            });
        }
    }

    if all_paths.is_empty() {
        eprintln!("ry: no .R / .r files found in {:?}", search_roots);
        return Ok(ExitCode::SUCCESS);
    }

    // Run the initial check.
    let result = run_check_once(
        &all_paths,
        &filter,
        format,
        &cfg.packages,
        &cfg.globals,
        color,
    )?;
    result.print_summary(format, statistics);

    if !watch {
        return Ok(result.exit_code(&cfg));
    }
    if !matches!(
        format,
        ry_checker::format::OutputFormat::Full | ry_checker::format::OutputFormat::Concise
    ) {
        eprintln!("ry: --watch requires the full or concise output format");
        return Ok(ExitCode::FAILURE);
    }

    // Watch mode: poll for changes and re-check.
    eprintln!(
        "ry: watching {} file(s) for changes (Ctrl+C to stop)...",
        all_paths.len()
    );
    let mut stamps: HashMap<PathBuf, std::time::SystemTime> = HashMap::new();
    for p in &all_paths {
        if let Ok(meta) = std::fs::metadata(p) {
            if let Ok(mtime) = meta.modified() {
                stamps.insert(p.clone(), mtime);
            }
        }
    }

    let poll_interval = std::time::Duration::from_millis(500);
    loop {
        std::thread::sleep(poll_interval);

        // Re-scan for new/deleted files.
        let mut current_paths = Vec::new();
        for root in &search_roots {
            collect_r_files(root, &mut current_paths);
        }
        sort_and_deduplicate_paths(&mut current_paths);
        if let Some(root) = config_root.as_ref() {
            if !excludes.is_empty() {
                current_paths.retain(|p| {
                    let rel = relative_path_for_exclude(p, root);
                    !excludes.matches(&rel)
                });
            }
        }

        // Check for any file modification or file set change.
        let mut changed = current_paths.len() != all_paths.len();
        if !changed {
            if current_paths != all_paths {
                changed = true;
            }
        }
        if !changed {
            for p in &current_paths {
                if let Ok(meta) = std::fs::metadata(p) {
                    if let Ok(mtime) = meta.modified() {
                        let prev = stamps.get(p).copied();
                        if prev != Some(mtime) {
                            changed = true;
                            stamps.insert(p.clone(), mtime);
                            break;
                        }
                    }
                }
            }
        }

        if changed {
            all_paths = current_paths;
            // Re-sync stamps for any new files.
            for p in &all_paths {
                if let Ok(meta) = std::fs::metadata(p) {
                    if let Ok(mtime) = meta.modified() {
                        stamps.insert(p.clone(), mtime);
                    }
                }
            }
            // Clear screen for a clean view of the new diagnostics.
            // Using ANSI escape sequences rather than `clear` command
            // for portability (no external process spawn).
            eprint!("\x1b[2J\x1b[H");
            let result = run_check_once(
                &all_paths,
                &filter,
                format,
                &cfg.packages,
                &cfg.globals,
                color,
            )?;
            result.print_summary(format, statistics);
        }
    }
}

/// Result of a single check pass: the diagnostics, file count, and
/// parse error count. Used by both one-shot and watch mode to print
/// results and compute the exit code.
struct CheckResult {
    diagnostics: Vec<ry_checker::Diagnostic>,
    file_count: usize,
    parse_errors: usize,
}

impl CheckResult {
    fn print_summary(&self, format: ry_checker::format::OutputFormat, statistics: bool) {
        // Suppress the human summary line for machine-readable formats
        // so it can't corrupt JSON/Github/Gitlab/Junit output (it goes
        // to stderr, but consumers that merge stderr would see it). The
        // plan calls for printing it only for the human formats.
        let is_human = matches!(
            format,
            ry_checker::format::OutputFormat::Full | ry_checker::format::OutputFormat::Concise
        );
        if !is_human && !statistics {
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
            return;
        }
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
        eprintln!(
            "ry: checked {} file(s), {} error(s), {} warning(s)",
            self.file_count, errors, warnings
        );
    }

    fn exit_code(&self, cfg: &config::Config) -> ExitCode {
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
        let failed = errors > 0 || self.parse_errors > 0 || (cfg.error_on_warning && warnings > 0);
        if cfg.exit_zero || !failed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

/// Core check logic: parse all files, run the project checker, apply
/// the severity filter, print diagnostics, and return a summary. Used
/// by both one-shot `ry check` and `ry check --watch` iterations.
fn run_check_once(
    all_paths: &[PathBuf],
    filter: &ry_checker::SeverityFilter,
    format: ry_checker::format::OutputFormat,
    packages: &[String],
    globals: &[String],
    color: bool,
) -> Result<CheckResult> {
    let mut all_diagnostics: Vec<ry_checker::Diagnostic> = Vec::new();
    let mut srcs: HashMap<String, String> = HashMap::new();
    let mut comments: HashMap<String, Vec<ry_core::ast::Comment>> = HashMap::new();
    let mut parse_errors = 0usize;
    let mut file_count = 0usize;

    // Multi-file project mode: build a single `Project` so functions
    // defined in one file are visible when checking another.
    let mut project = ry_checker::Project::new();

    // Parallel file parsing. tree-sitter parsers are
    // NOT `Send`, so each rayon thread keeps its own `RParser` in a
    // `thread_local!` (the grammar is loaded once per thread; the
    // thread pool is reused across this run). Parsed files come back in
    // arbitrary thread order; we re-sort to input path order for stable
    // diagnostic output. The single-parser optimization (reusing one
    // parser across documents) is preserved within each thread.
    thread_local! {
        static PARSER: std::cell::RefCell<Option<ry_core::RParser>> =
            const { std::cell::RefCell::new(None) };
    }
    let parse_one = |path: &std::path::Path| -> Result<(String, String, ry_core::SourceFile), ()> {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("ry: {}: {}", path.display(), e);
                return Err(());
            }
        };
        let path_str = path.to_string_lossy().to_string();
        let file = PARSER.with(|cell| {
            let mut slot = cell.borrow_mut();
            let parser = slot.get_or_insert_with(|| {
                ry_core::RParser::new().expect("parser init (thread-local)")
            });
            parser.parse(&path_str, &src)
        });
        match file {
            Ok(f) => Ok((path_str, src, f)),
            Err(e) => {
                eprintln!("ry: {}: parse error: {}", path.display(), e);
                Err(())
            }
        }
    };
    // Parallel collect, tracking input index for a stable re-sort.
    use rayon::prelude::*;
    let mut parsed: Vec<(usize, String, String, ry_core::SourceFile)> = all_paths
        .par_iter()
        .enumerate()
        .filter_map(|(i, path)| parse_one(path).ok().map(|(p, s, f)| (i, p, s, f)))
        .collect();
    parse_errors += all_paths.len() - parsed.len();
    parsed.sort_by_key(|(i, _, _, _)| *i);
    let package_scope = package_metadata::resolve(
        all_paths,
        packages,
        globals,
        parsed.iter().map(|(_, _, _, file)| file),
    );
    for (_, path_str, src, file) in parsed {
        project.add_file(path_str.clone(), file.clone());
        srcs.insert(path_str.clone(), src);
        comments.insert(path_str.clone(), file.comments);
        file_count += 1;
    }

    project.set_loaded(package_scope.attached);
    project.set_external_bindings(package_scope.bindings);
    project.set_imported_from(package_scope.imported_from);
    project.set_external_s3_methods(package_scope.s3_methods);
    project.set_load_bindings(package_scope.load_bindings);

    let mut per_file_diagnostics = project.check();

    // Apply inline suppression comments (`# ry: ignore`, `# noqa`,
    // `# ry: ignore-file`) before the severity filter so a suppressed
    // error never even reaches the filter pipeline. Use the lexical
    // (comment-based) filter so a `#` inside a string literal is not
    // mistaken for a suppression directive.
    for (path, diags) in &mut per_file_diagnostics {
        if let Some(cs) = comments.get(path) {
            *diags = ry_checker::filter_suppressed_with_comments(std::mem::take(diags), cs);
        }
    }

    for (_path, diags) in &mut per_file_diagnostics {
        ry_checker::apply_filter_to_diagnostics(diags, filter);
    }
    for (_path, diags) in per_file_diagnostics {
        all_diagnostics.extend(diags);
    }

    sort_and_deduplicate_diagnostics(&mut all_diagnostics);

    let rendered = ry_checker::format::render_with_color(&all_diagnostics, format, &srcs, color);
    if !rendered.is_empty() {
        // Diagnostics go to STDOUT (matches ruff/ty): `ry check > log`
        // captures the diagnostics, while the summary line and watch-
        // mode chrome go to stderr. Machine formats (json/github/...)
        // already used stdout; human formats (concise/full) now do too.
        print!("{}", rendered);
    }

    Ok(CheckResult {
        diagnostics: all_diagnostics,
        file_count,
        parse_errors,
    })
}

fn sort_and_deduplicate_diagnostics(diagnostics: &mut Vec<ry_checker::Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.line.cmp(&b.span.line))
            .then(a.span.col.cmp(&b.span.col))
            .then(a.span.start.cmp(&b.span.start))
            .then(a.span.end.cmp(&b.span.end))
            .then(a.code.cmp(b.code))
            .then(a.severity.as_str().cmp(b.severity.as_str()))
            .then(a.message.cmp(&b.message))
    });
    diagnostics.dedup_by(|a, b| {
        a.path == b.path
            && a.span == b.span
            && a.code == b.code
            && a.severity == b.severity
            && a.message == b.message
    });
}

fn print_version(format: &str) {
    let v = env!("CARGO_PKG_VERSION");
    match format {
        "json" => println!("{{\"name\":\"ry\",\"version\":\"{}\"}}", v),
        _ => println!("ry {}", v),
    }
}

fn run_explain_rule(rule: Option<String>, output_format: &str) -> Result<ExitCode> {
    let rules = ry_checker::rules::RULES;
    let matched: Vec<&'static ry_checker::rules::Rule> = match &rule {
        Some(name) => match ry_checker::rules::find(name) {
            Some(r) => vec![r],
            None => {
                eprintln!("ry: unknown rule `{}`", name);
                return Ok(ExitCode::FAILURE);
            }
        },
        None => rules.iter().collect(),
    };
    match output_format {
        "json" => {
            let json: Vec<serde_json::Value> = matched
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "code": r.code,
                        "name": r.name,
                        "severity": r.default_severity.as_str(),
                        "summary": r.summary,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json).unwrap());
        }
        _ => {
            if matched.len() == 1 {
                let r = matched[0];
                println!("{} ({})", r.code, r.name);
                println!("Default severity: {}", r.default_severity);
                println!();
                println!("{}", r.summary);
            } else {
                println!("{:<8} {:<24} {:<10} summary", "code", "name", "severity");
                println!("{}", "-".repeat(78));
                for r in &matched {
                    println!(
                        "{:<8} {:<24} {:<10} {}",
                        r.code,
                        r.name,
                        r.default_severity.as_str(),
                        r.summary
                    );
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_explain_typeshed() -> Result<ExitCode> {
    let t = ry_typeshed::load_base().into_diagnostic()?;
    println!("version: {}", t.version);
    println!("functions: {}", t.functions.len());
    for (k, v) in &t.functions {
        println!("  {}({})", k, v.params.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_shell_completion(shell: &str) -> Result<ExitCode> {
    let mut cmd = Cli::command();
    let shell_kind = match shell.to_ascii_lowercase().as_str() {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "elvish" => clap_complete::Shell::Elvish,
        "powershell" | "pwsh" => clap_complete::Shell::PowerShell,
        other => {
            eprintln!("ry: unknown shell `{}`", other);
            return Ok(ExitCode::FAILURE);
        }
    };
    clap_complete::generate(shell_kind, &mut cmd, "ry", &mut std::io::stdout());
    Ok(ExitCode::SUCCESS)
}

fn collect_r_files(path: &std::path::Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            // Skip hidden / VCS / target dirs.
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            collect_r_files(&p, out);
        } else if matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("R") | Some("r")
        ) {
            out.push(p);
        }
    }
}

fn sort_and_deduplicate_paths(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}

#[cfg(test)]
mod tests {
    use super::{ColorChoice, sort_and_deduplicate_diagnostics};
    use ry_checker::format::OutputFormat;
    use ry_checker::{Diagnostic, Severity};
    use ry_core::Span;

    fn diag(path: &str, line: usize, col: usize, code: &'static str) -> Diagnostic {
        Diagnostic::new(
            Severity::Warning,
            Span::new(line * 10 + col, line * 10 + col + 1, line, col),
            path,
            code,
            "same message",
        )
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
    fn color_policy_covers_terminal_no_color_and_machine_formats() {
        assert!(ColorChoice::Auto.enabled_for(OutputFormat::Full, true, false));
        assert!(!ColorChoice::Auto.enabled_for(OutputFormat::Full, true, true));
        assert!(!ColorChoice::Auto.enabled_for(OutputFormat::Concise, false, false));
        assert!(!ColorChoice::Never.enabled_for(OutputFormat::Full, true, false));

        for format in [
            OutputFormat::Json,
            OutputFormat::Github,
            OutputFormat::Gitlab,
            OutputFormat::Junit,
        ] {
            assert!(!ColorChoice::Always.enabled_for(format, true, false));
        }
    }
}
