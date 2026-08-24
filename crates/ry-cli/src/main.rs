#![allow(clippy::collapsible_if)]

mod check;

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::parser::ValueSource;
use clap::{
    ArgMatches, CommandFactory, FromArgMatches, Parser as ClapParser, Subcommand, ValueEnum,
};
use miette::{IntoDiagnostic, Result};

use ry_config as config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ConfidenceChoice {
    Low,
    Medium,
    High,
}

impl From<ConfidenceChoice> for ry_checker::Confidence {
    fn from(value: ConfidenceChoice) -> Self {
        match value {
            ConfidenceChoice::Low => Self::Low,
            ConfidenceChoice::Medium => Self::Medium,
            ConfidenceChoice::High => Self::High,
        }
    }
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
        /// Load package stubs from this directory. Repeatable; later
        /// directories replace same-named packages from earlier ones.
        #[arg(long, value_name = "DIR")]
        typeshed: Vec<PathBuf>,
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
        /// Write the current diagnostics as a line-number-free JSON baseline.
        #[arg(long, value_name = "PATH", conflicts_with = "baseline")]
        write_baseline: Option<PathBuf>,
        /// Suppress diagnostics matching entries in this baseline file.
        #[arg(long, value_name = "PATH")]
        baseline: Option<PathBuf>,
        /// Only show diagnostics at or above this confidence tier.
        #[arg(long, value_enum, default_value_t = ConfidenceChoice::Low)]
        min_confidence: ConfidenceChoice,
    },
    /// Dump inferred types for every lexical scope in R files, as JSON on
    /// stdout. Non-interactive counterpart of the LSP's inline type
    /// hints: bindings map to the same type strings. Downstream tooling
    /// (training-data builders, IDE backends) can query which names a
    /// scope binds and with what inferred types, without re-implementing
    /// the checker.
    DumpTypes {
        /// R files or directories to dump. A directory expands to every
        /// discoverable R file under it, using `ry check`'s discovery
        /// rules.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Analysis root for whole-project inference. Defaults, per file,
        /// to the nearest ancestor directory containing a DESCRIPTION
        /// (the enclosing package), else the directory owning the
        /// discovered ry.toml, else the current directory — the same
        /// grouping `ry check` uses.
        #[arg(long, value_name = "DIR")]
        project_root: Option<PathBuf>,
        /// Output format. Only `json` is supported.
        #[arg(long, value_name = "FORMAT", default_value = "json")]
        format: String,
        /// Restrict output to the innermost scope(s) containing this
        /// position, as 1-based LINE:COL. Repeatable; every position is
        /// evaluated against every dumped file.
        #[arg(long = "position", value_name = "LINE:COL", value_parser = parse_dump_position)]
        positions: Vec<(usize, usize)>,
    },
    /// Start the language server. Speaks the Language Server Protocol
    /// (LSP) over stdio, publishing type-check diagnostics for open R
    /// files. Connect to it from any LSP-aware editor (VS Code, Neovim,
    /// Helix, etc.).
    Server {
        /// Tracing filter for the LSP server. Passed to
        /// `tracing_subscriber`'s `EnvFilter`. Defaults to `ry=warn`.
        /// Examples: `ry=debug`, `ry_lsp=trace`, `warn`.
        #[arg(long, default_value = "ry=warn")]
        log_level: String,
    },
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
    /// Explain analyzer data and configuration.
    Explain {
        #[command(subcommand)]
        command: ExplainCmd,
    },
    /// Work with R package typeshed files.
    Typeshed {
        #[command(subcommand)]
        command: TypeshedCmd,
    },
    /// Show the embedded typeshed (debug).
    #[command(hide = true)]
    ExplainTypeshed,
    /// Generate shell completions.
    GenerateShellCompletion {
        /// Target shell.
        shell: String,
    },
}

#[derive(Debug, Subcommand)]
enum ExplainCmd {
    /// Explain a rule (or all rules).
    Rule {
        /// Rule code or name. Omit to list all rules.
        rule: Option<String>,
        /// Output format: text or json.
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: String,
    },
    /// Show vendored and active runtime typeshed packages.
    Typeshed,
}

#[derive(Debug, Subcommand)]
enum TypeshedCmd {
    /// Validate stub files with ry's normative typeshed parser.
    Validate {
        /// Directories containing flat or per-package stub files.
        #[arg(value_name = "DIR", required = true)]
        dirs: Vec<PathBuf>,
    },
}

fn main() -> Result<ExitCode> {
    // `ArgMatches` is kept alongside the typed `Cli` so `flag_set` can
    // tell a user-passed flag from its clap default (see `flag_set`).
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
            typeshed: Vec::new(),
            error_on_warning: false,
            exit_zero: false,
            output_format: "full".to_string(),
            color: ColorChoice::Auto,
            watch: false,
            statistics: false,
            write_baseline: None,
            baseline: None,
            min_confidence: ConfidenceChoice::Low,
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
        } => run_check(
            paths,
            error,
            warn,
            ignore,
            typeshed,
            error_on_warning,
            exit_zero,
            &output_format,
            color,
            cli.verbose,
            cli.quiet,
            check_matches,
            watch,
            statistics,
            write_baseline,
            baseline,
            min_confidence,
        ),
        Cmd::DumpTypes {
            files,
            project_root,
            format,
            positions,
        } => run_dump_types(files, project_root, &format, positions),
        Cmd::Server { log_level } => {
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
            let filter = tracing_subscriber::EnvFilter::try_new(&log_level)
                .map_err(|e| miette::miette!("invalid --log-level '{log_level}': {e}"))?;
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(filter)
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
        Cmd::Explain { command } => match command {
            ExplainCmd::Rule {
                rule,
                output_format,
            } => run_explain_rule(rule, &output_format),
            ExplainCmd::Typeshed => run_explain_typeshed(),
        },
        Cmd::Typeshed {
            command: TypeshedCmd::Validate { dirs },
        } => run_typeshed_validate(&dirs, cli.quiet > 0),
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

/// Returns true if the named argument was explicitly provided on the
/// command line (rather than coming from a clap default value). Used to
/// distinguish "the user passed `--error-on-warning`" from "the field's
/// default of false", which is what lets the `ry.toml` value take
/// effect when the CLI flag is omitted.
fn flag_set(matches: Option<&ArgMatches>, id: &str) -> bool {
    matches.and_then(|m| m.value_source(id)) == Some(ValueSource::CommandLine)
}

fn demote_non_source_paths(
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
                if let Ok(relative) = absolute.strip_prefix(root) {
                    if relative.components().any(|component| {
                        component
                            .as_os_str()
                            .to_str()
                            .is_some_and(|name| DEMOTED.contains(&name))
                    }) {
                        diagnostic.confidence = diagnostic.confidence.demote();
                    }
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
    if matches!(
        format,
        ry_checker::format::OutputFormat::Full | ry_checker::format::OutputFormat::Concise
    ) {
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
#[allow(clippy::too_many_arguments)]
fn run_check(
    paths: Vec<PathBuf>,
    error: Vec<String>,
    warn: Vec<String>,
    ignore: Vec<String>,
    typeshed: Vec<PathBuf>,
    error_on_warning: bool,
    exit_zero: bool,
    output_format: &str,
    color: ColorChoice,
    cli_verbose: u8,
    cli_quiet: u8,
    check_matches: Option<&ArgMatches>,
    watch: bool,
    statistics: bool,
    write_baseline: Option<PathBuf>,
    baseline: Option<PathBuf>,
    min_confidence: ConfidenceChoice,
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

    // Forward `None` for scalars the CLI did not set explicitly, so the
    // config file's value wins.
    let m = check_matches;
    let cli_error_on_warning = flag_set(m, "error_on_warning").then_some(error_on_warning);
    let cli_exit_zero = flag_set(m, "exit_zero").then_some(exit_zero);
    let cli_output_format = flag_set(m, "output_format").then_some(output_format.to_string());
    let baseline_from_cli = flag_set(m, "baseline");

    let cfg = base_cfg.merge_cli(
        error,
        warn,
        ignore,
        typeshed,
        baseline,
        cli_error_on_warning,
        cli_exit_zero,
        cli_output_format,
        cli_verbose,
        cli_quiet,
    );

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
    // module (P36-W7 / issue #48). CLI and LSP use the same eligibility,
    // extension, hidden-directory, symlink, exclude, and test-fixture rules.
    let mut all_paths = Vec::new();
    let search_roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths
    };
    for root in &search_roots {
        let result = ry_workspace::discover_r_files(
            root,
            config_root.as_deref(),
            &cfg,
            cfg.check_test_fixtures,
        );
        all_paths.extend(result.files);
        report_truncation(&result.truncated, root);
    }
    sort_and_deduplicate_paths(&mut all_paths);

    if all_paths.is_empty() {
        eprintln!("ry: no .R / .r files found in {:?}", search_roots);
        return Ok(ExitCode::SUCCESS);
    }

    let result = run_check_once(
        &all_paths,
        &filter,
        format,
        &cfg,
        Arc::clone(&user_stubs),
        color,
        baseline.as_ref(),
        config_root.as_deref(),
        min_confidence.into(),
    )?;
    if let Some(path) = write_baseline.as_deref() {
        config::write_baseline_file(path, &result.diagnostics, config_root.as_deref())?;
    }
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

        // Re-scan for new/deleted files via shared bounded discovery.
        let mut current_paths = Vec::new();
        for root in &search_roots {
            let result = ry_workspace::discover_r_files(
                root,
                config_root.as_deref(),
                &cfg,
                cfg.check_test_fixtures,
            );
            current_paths.extend(result.files);
        }
        sort_and_deduplicate_paths(&mut current_paths);

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
                &cfg,
                Arc::clone(&user_stubs),
                color,
                baseline.as_ref(),
                config_root.as_deref(),
                min_confidence.into(),
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
/// Keep the original majority-error guard, and also catch foreign files whose
/// syntax happens to produce many recoverable R expressions.  The absolute
/// floor avoids suppressing ordinary R files with a few syntax errors.
fn is_probably_not_r_source(file: &ry_core::SourceFile) -> bool {
    let parse_errors = file.parse_errors.len();
    let statements = file.stmts.len();

    parse_errors > statements || (parse_errors >= 5 && parse_errors * 100 >= 15 * statements.max(1))
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
            self.print_degraded();
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
        self.print_degraded();
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
#[allow(clippy::too_many_arguments)]
fn run_check_once(
    all_paths: &[PathBuf],
    filter: &ry_checker::SeverityFilter,
    format: ry_checker::format::OutputFormat,
    resolution_config: &config::Config,
    user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    color: bool,
    baseline: Option<&config::Baseline>,
    repo_root: Option<&std::path::Path>,
    min_confidence: ry_checker::Confidence,
) -> Result<CheckResult> {
    let mut all_diagnostics: Vec<ry_checker::Diagnostic> = Vec::new();
    let mut srcs: HashMap<String, String> = HashMap::new();
    let mut comments: HashMap<String, Vec<ry_core::ast::Comment>> = HashMap::new();
    let mut parse_errors = 0usize;
    let mut file_count = 0usize;
    let mut not_r_diagnostics = Vec::new();
    // Degraded scopes (serialized data over the byte cap), deduplicated and
    // sorted for a stable summary. Keyed on the formatted `path (reason)`.
    let mut degraded: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

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
        let src = match read_r_source(path) {
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
    parsed.retain(|(_, path, src, file)| {
        file_count += 1;
        srcs.insert(path.clone(), src.clone());
        if is_probably_not_r_source(file) {
            not_r_diagnostics.push(ry_checker::Diagnostic::new(
                ry_checker::Severity::Info,
                ry_core::Span::new(0, 1, 0, 0),
                path,
                "RY097",
                "File does not appear to be R source; diagnostics suppressed.",
            ));
            false
        } else {
            true
        }
    });
    // Each R package is a separate library scope. Pooling multiple package
    // roots into one Project lets top-level bindings and inferred functions
    // leak between namespaces, which can both hide real RY010 findings and
    // activate the wrong NSE model. Non-package scripts remain one project so
    // ordinary multi-file workflows keep their source()-style visibility.
    let mut groups: std::collections::BTreeMap<Option<PathBuf>, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, (_, path, _, _)) in parsed.iter().enumerate() {
        groups
            .entry(enclosing_package_root(std::path::Path::new(path)))
            .or_default()
            .push(index);
    }

    let mut per_file_diagnostics = Vec::new();
    for (group_root, indices) in &groups {
        let resolution_root = group_root
            .clone()
            .or_else(|| repo_root.map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let package_scope = ry_workspace::resolve_workspace_context(
            &resolution_root,
            resolution_config,
            ry_workspace::ResolutionEnvironment {
                files: indices.iter().map(|index| &parsed[*index].3).collect(),
                user_stubs: &user_stubs,
            },
        )
        .map_err(|error| miette::miette!(error))?;
        let mut analysis_files = Vec::new();
        for index in indices {
            let (_, path, _, file) = &parsed[*index];
            analysis_files.push((path.clone(), 0, std::sync::Arc::new(file.clone())));
            comments.insert(path.clone(), file.comments.clone());
        }
        let check_input = check::CheckInput {
            files: analysis_files,
            user_stubs: Arc::clone(&user_stubs),
            workspace: Some(ry_workspace::WorkspaceContext {
                attached_packages: package_scope.attached_packages,
                bare_bindings: package_scope.bare_bindings,
                external_bindings: package_scope.external_bindings,
                imported_bindings: package_scope.imported_bindings,
                s3_methods: package_scope.s3_methods,
                load_bindings: package_scope.load_bindings,
                degraded_scopes: Vec::new(),
            }),
        };
        let check_output = check::check_project(check_input);
        per_file_diagnostics.extend(check_output.diagnostics);
        for (path, reason) in package_scope.degraded_scopes {
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
        ry_checker::apply_filter_to_diagnostics(diags, filter);
    }
    ry_checker::apply_filter_to_diagnostics(&mut not_r_diagnostics, filter);
    all_diagnostics.append(&mut not_r_diagnostics);
    for (_path, diags) in per_file_diagnostics {
        all_diagnostics.extend(diags);
    }

    demote_non_source_paths(&mut all_diagnostics, repo_root);
    if let Some(baseline) = baseline {
        config::subtract_baseline(&mut all_diagnostics, baseline, repo_root);
    }
    all_diagnostics.retain(|diagnostic| diagnostic.confidence >= min_confidence);

    sort_and_deduplicate_diagnostics(&mut all_diagnostics);

    let rendered = render_diagnostics(&all_diagnostics, format, &srcs, color);
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
        degraded: degraded.into_iter().collect(),
    })
}

fn load_user_stubs(
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

// ---------------------------------------------------------------------------
// dump-types
//
// One analysis pass over the requested files, then a JSON dump of every
// recorded lexical scope (see ry_checker::ScopeRecord). The dump reuses
// `ry check`'s pipeline end to end — config discovery, file discovery,
// per-package grouping, workspace resolution — so the emitted types are
// exactly what a `ry check` run infers.

#[derive(serde::Serialize)]
struct TypesDump {
    files: Vec<FileDump>,
}

#[derive(serde::Serialize)]
struct FileDump {
    path: String,
    scopes: Vec<ScopeDump>,
}

#[derive(serde::Serialize)]
struct ScopeDump {
    kind: &'static str,
    name: Option<String>,
    start: (usize, usize),
    end: (usize, usize),
    bindings: Vec<BindingDump>,
}

#[derive(serde::Serialize)]
struct BindingDump {
    name: String,
    kind: &'static str,
    #[serde(rename = "type")]
    type_: String,
    start: Option<(usize, usize)>,
}

/// clap value parser for `--position LINE:COL`. Rows and columns are
/// 1-based, matching the dump output.
fn parse_dump_position(value: &str) -> Result<(usize, usize), String> {
    let (line, col) = value
        .split_once(':')
        .ok_or_else(|| format!("expected LINE:COL, got `{value}`"))?;
    let line = line
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid line in `{value}`"))?;
    let col = col
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid column in `{value}`"))?;
    if line == 0 || col == 0 {
        return Err(format!("positions are 1-based, got `{value}`"));
    }
    Ok((line, col))
}

/// The type string for one binding. Same `Display` rendering the LSP
/// inlay hints show, except the fully-uninformed type is reported as
/// "unknown" so consumers never mistake `opaque<len=?>:?` for a real
/// inference result.
fn dump_type_string(t: &ry_core::RType) -> String {
    if *t == ry_core::RType::unknown() {
        "unknown".to_string()
    } else {
        t.to_string()
    }
}

/// 1-based (row, character-column) of a byte offset. Parser spans use
/// byte columns; converting to character columns keeps the dump useful
/// for files with multi-byte identifiers.
fn offset_to_line_char_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let row = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = source[line_start..offset].chars().count() + 1;
    (row, col)
}

/// Inverse of [`offset_to_line_char_col`]: byte offset of the 1-based
/// (row, character-column) position. `None` when the row is past the
/// last line; a column past the line end clamps to the line end.
fn line_char_col_to_offset(source: &str, row: usize, col: usize) -> Option<usize> {
    let mut offset = 0usize;
    for (index, line) in source.split('\n').enumerate() {
        if index + 1 == row {
            let mut bytes = 0usize;
            for ch in line.chars().take(col.saturating_sub(1)) {
                bytes += ch.len_utf8();
            }
            return Some((offset + bytes).min(offset + line.len()));
        }
        offset += line.len() + 1;
    }
    None
}

/// Record the first plain assignment to each name in a scope's body.
///
/// R has no separate block scoping, so assignments inside `if`/`for`/
/// `while` bodies and braced value blocks bind in the enclosing function
/// scope. Function-literal bodies are excluded: those are their own
/// scopes (recorded separately), though the *name* bound to a function
/// literal (`inner <- function(...)`) is itself a local of this scope.
fn collect_local_bindings(stmts: &[ry_core::ast::Stmt], out: &mut HashMap<String, ry_core::Span>) {
    for statement in stmts {
        match statement {
            ry_core::ast::Stmt::Assign { target, value, .. } => {
                match target {
                    ry_core::ast::Expr::Ident { name, span }
                    | ry_core::ast::Expr::String(name, span) => {
                        out.entry(name.clone()).or_insert(*span);
                    }
                    // Indexed targets (`d$col <- v`) mutate an existing
                    // binding rather than creating one; the base name's
                    // plain assignment (if any) is found elsewhere. The
                    // target's index arguments are not walked either: the
                    // checker does not bind assignments hidden there
                    // (`m[i <- 1L] <- v` leaves `i` unbound), and the dump
                    // reports exactly what a `ry check` run infers.
                    _ => {}
                }
                collect_local_bindings_in_expr(value, out);
            }
            ry_core::ast::Stmt::Expr(e) => collect_local_bindings_in_expr(e, out),
            ry_core::ast::Stmt::If {
                cond, then, else_, ..
            } => {
                collect_local_bindings_in_expr(cond, out);
                collect_local_bindings(then, out);
                if let Some(else_) = else_ {
                    collect_local_bindings(else_, out);
                }
            }
            ry_core::ast::Stmt::For {
                name,
                name_span,
                iter,
                body,
                ..
            } => {
                out.entry(name.clone()).or_insert(*name_span);
                collect_local_bindings_in_expr(iter, out);
                collect_local_bindings(body, out);
            }
            ry_core::ast::Stmt::While { cond, body, .. } => {
                collect_local_bindings_in_expr(cond, out);
                collect_local_bindings(body, out);
            }
            // `return(e)` still binds any assignment inside `e`; a bare
            // function definition binds no name in this scope.
            ry_core::ast::Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    collect_local_bindings_in_expr(value, out);
                }
            }
            ry_core::ast::Stmt::FunctionDef { .. } => {}
        }
    }
}

/// Statement-level assignment scan through expression positions. R nests
/// both statements (braced blocks) and side-effecting assignments
/// (`f(x <- 1L)`, chained `a <- b <- 1L`, `if (flag <- f())`) inside
/// arbitrary expressions, and all of them bind in the enclosing function
/// scope. The recursion set mirrors the checker's own expression walker
/// (`ry_checker::infer`'s `visit_expr`); literal leaves bind nothing.
/// Function-literal bodies are excluded: those are their own scopes
/// (recorded separately), though the *name* bound to a function literal
/// (`inner <- function(...)`) is itself a local of this scope.
fn collect_local_bindings_in_expr(
    expr: &ry_core::ast::Expr,
    out: &mut HashMap<String, ry_core::Span>,
) {
    match expr {
        ry_core::ast::Expr::Call { func, args, .. } => {
            collect_local_bindings_in_expr(func, out);
            for argument in args {
                collect_local_bindings_in_expr(&argument.value, out);
            }
        }
        // Assignment operators in expression position bind the LHS in
        // the current scope — the checker's `Expr::BinOp` arm does the
        // same for `<-`/`<<-` (R's `<-` returns the value invisibly) and
        // `%<>%` (which rebinds its LHS ident).
        ry_core::ast::Expr::BinOp {
            op:
                ry_core::ast::BinOpKind::Assign
                | ry_core::ast::BinOpKind::SuperAssign
                | ry_core::ast::BinOpKind::PipeAssign,
            lhs,
            rhs,
            ..
        } => {
            match lhs.as_ref() {
                ry_core::ast::Expr::Ident { name, span }
                | ry_core::ast::Expr::String(name, span) => {
                    out.entry(name.clone()).or_insert(*span);
                }
                // Replacement-function targets (`names(d) <- v`) and
                // indexed targets mutate; the checker's assignment arms
                // decide those, not this walk.
                _ => {}
            }
            collect_local_bindings_in_expr(lhs, out);
            collect_local_bindings_in_expr(rhs, out);
        }
        ry_core::ast::Expr::BinOp { lhs, rhs, .. } => {
            collect_local_bindings_in_expr(lhs, out);
            collect_local_bindings_in_expr(rhs, out);
        }
        ry_core::ast::Expr::UnaryOp { expr, .. } => collect_local_bindings_in_expr(expr, out),
        ry_core::ast::Expr::Index { base, args, .. } => {
            collect_local_bindings_in_expr(base, out);
            for argument in args {
                collect_local_bindings_in_expr(&argument.value, out);
            }
        }
        ry_core::ast::Expr::Block { body, .. } => collect_local_bindings(body, out),
        ry_core::ast::Expr::If {
            then, else_, cond, ..
        } => {
            collect_local_bindings_in_expr(cond, out);
            collect_local_bindings_in_expr(then, out);
            if let Some(else_) = else_ {
                collect_local_bindings_in_expr(else_, out);
            }
        }
        // Function literals are separate scopes; their bodies are indexed
        // by index_scope_bodies instead. Literals and `Unknown` bind
        // nothing.
        _ => {}
    }
}

/// Map every function body in the file to its start byte, so each
/// recorded scope can look up its own local bindings. Mirrors the
/// statement recursion of `collect_local_bindings` to find nested named
/// functions at any block depth.
fn index_scope_bodies(
    stmts: &[ry_core::ast::Stmt],
    index: &mut HashMap<usize, HashMap<String, ry_core::Span>>,
) {
    for statement in stmts {
        match statement {
            ry_core::ast::Stmt::Assign { value, .. } => match value {
                ry_core::ast::Expr::Function { body, span, .. } => {
                    index_function_body(*span, body, index);
                }
                _ => index_scope_bodies_in_expr(value, index),
            },
            ry_core::ast::Stmt::FunctionDef { body, span, .. } => {
                index_function_body(*span, body, index);
            }
            ry_core::ast::Stmt::If {
                cond, then, else_, ..
            } => {
                index_scope_bodies_in_expr(cond, index);
                index_scope_bodies(then, index);
                if let Some(else_) = else_ {
                    index_scope_bodies(else_, index);
                }
            }
            ry_core::ast::Stmt::For { iter, body, .. } => {
                index_scope_bodies_in_expr(iter, index);
                index_scope_bodies(body, index);
            }
            ry_core::ast::Stmt::While { cond, body, .. } => {
                index_scope_bodies_in_expr(cond, index);
                index_scope_bodies(body, index);
            }
            ry_core::ast::Stmt::Expr(e) => index_scope_bodies_in_expr(e, index),
            ry_core::ast::Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    index_scope_bodies_in_expr(value, index);
                }
            }
        }
    }
}

/// Same expression recursion as [`collect_local_bindings_in_expr`]: named
/// functions are reachable through any expression position (a callback's
/// body, a call argument, an index argument), and only finding them there
/// gives their scopes `function_locals` entries.
fn index_scope_bodies_in_expr(
    expr: &ry_core::ast::Expr,
    index: &mut HashMap<usize, HashMap<String, ry_core::Span>>,
) {
    match expr {
        ry_core::ast::Expr::Call { func, args, .. } => {
            index_scope_bodies_in_expr(func, index);
            for argument in args {
                index_scope_bodies_in_expr(&argument.value, index);
            }
        }
        ry_core::ast::Expr::BinOp { lhs, rhs, .. } => {
            index_scope_bodies_in_expr(lhs, index);
            index_scope_bodies_in_expr(rhs, index);
        }
        ry_core::ast::Expr::UnaryOp { expr, .. } => index_scope_bodies_in_expr(expr, index),
        ry_core::ast::Expr::Index { base, args, .. } => {
            index_scope_bodies_in_expr(base, index);
            for argument in args {
                index_scope_bodies_in_expr(&argument.value, index);
            }
        }
        ry_core::ast::Expr::Block { body, .. } => index_scope_bodies(body, index),
        ry_core::ast::Expr::If {
            then, else_, cond, ..
        } => {
            index_scope_bodies_in_expr(cond, index);
            index_scope_bodies_in_expr(then, index);
            if let Some(else_) = else_ {
                index_scope_bodies_in_expr(else_, index);
            }
        }
        // An anonymous function literal gets no `ScopeRecord` of its own
        // (the checker infers it in discarding mode), but named functions
        // defined inside it complete and are recorded — walk the body so
        // those nested definitions are indexed. The literal itself needs
        // no `function_locals` entry: no record will ever look it up.
        ry_core::ast::Expr::Function { body, .. } => index_scope_bodies(body, index),
        _ => {}
    }
}

fn index_function_body(
    span: ry_core::Span,
    body: &[ry_core::ast::Stmt],
    index: &mut HashMap<usize, HashMap<String, ry_core::Span>>,
) {
    let mut locals = HashMap::new();
    collect_local_bindings(body, &mut locals);
    index.insert(span.start, locals);
    index_scope_bodies(body, index);
}

/// Per-scope classification inputs derived once per file.
struct ScopeInfo<'a> {
    record: &'a ry_checker::ScopeRecord,
    params: HashMap<&'a str, ry_core::Span>,
    locals: HashMap<&'a str, ry_core::Span>,
}

/// Turn one file's scope records into the JSON dump shape.
///
/// Binding kinds (documented in the README section for `dump-types`):
/// - `param`: still marked as a formal in the recorded scope (an
///   overwritten formal degrades to `local`, matching R's rebinding).
/// - `local`: first assigned inside this scope's own body.
/// - `closed-over`: function scopes only — present because the body's
///   scope was cloned from the enclosing one at definition time.
/// - `imported`: top-level bindings the file never assigns (ambient names
///   supplied by the host environment, e.g. Shiny server fragments).
fn assemble_file_dump(
    path: &str,
    file: &ry_core::SourceFile,
    records: Vec<ry_checker::ScopeRecord>,
    positions: &[(usize, usize)],
) -> FileDump {
    // Sort by start position and drop duplicates (an injected-expression
    // re-walk can complete the same literal twice). The dedup key
    // includes the kind: a leading function literal's span starts at the
    // same byte 0 as the whole-file top scope, and keying on the offset
    // alone would drop that top scope and every top-level binding with
    // it.
    let mut records = records;
    records.sort_by_key(|record| record.span.start);
    records.dedup_by(|a, b| a.kind == b.kind && a.span.start == b.span.start);

    let mut function_locals: HashMap<usize, HashMap<String, ry_core::Span>> = HashMap::new();
    index_scope_bodies(&file.stmts, &mut function_locals);
    let mut top_locals = HashMap::new();
    collect_local_bindings(&file.stmts, &mut top_locals);

    let infos: Vec<ScopeInfo> = records
        .iter()
        .map(|record| ScopeInfo {
            record,
            params: record
                .params
                .iter()
                .map(|(name, span)| (name.as_str(), *span))
                .collect(),
            locals: match record.kind {
                ry_checker::ScopeRecordKind::Function => function_locals
                    .get(&record.span.start)
                    .map(|locals| {
                        locals
                            .iter()
                            .map(|(name, span)| (name.as_str(), *span))
                            .collect()
                    })
                    .unwrap_or_default(),
                ry_checker::ScopeRecordKind::Top => top_locals
                    .iter()
                    .map(|(name, span)| (name.as_str(), *span))
                    .collect(),
            },
        })
        .collect();

    // Enclosing-scope chains are shared by every binding of a scope, so
    // compute them once per file instead of once per closed-over lookup.
    let chains = enclosing_scope_chains(&infos);

    // Which scopes does --position select? Without positions, all. With
    // them, the innermost containing scope for each position (the union,
    // deduplicated). Byte offsets make containment exact regardless of
    // encoding. One pass records both the selected scopes and, per
    // scope, the offsets that selected it — the latter drives
    // binding-visibility filtering below.
    let mut selected: Vec<usize> = Vec::new();
    let mut selecting_offsets: HashMap<usize, Vec<usize>> = HashMap::new();
    if positions.is_empty() {
        selected.extend(0..infos.len());
    } else {
        for &(row, col) in positions {
            let Some(offset) = line_char_col_to_offset(&file.source, row, col) else {
                continue;
            };
            let mut best: Option<(usize, usize)> = None;
            for (index, info) in infos.iter().enumerate() {
                let span = info.record.span;
                if span.start <= offset && offset < span.end {
                    let extent = span.end - span.start;
                    if best.is_none_or(|(extent_so_far, _)| extent < extent_so_far) {
                        best = Some((extent, index));
                    }
                }
            }
            if let Some((_, index)) = best {
                selecting_offsets.entry(index).or_default().push(offset);
            }
        }
        selected.extend(selecting_offsets.keys().copied());
        selected.sort_unstable();
        selected.dedup();
    }

    let scopes = selected
        .into_iter()
        .map(|index| {
            let info = &infos[index];
            let record = info.record;
            let source = &file.source;

            let mut bindings: Vec<BindingDump> = record
                .scope
                .bindings
                .iter()
                .map(|(name, ty)| {
                    // A formal counts as `param` only in the scope that
                    // declares it: scope snapshots are cloned from the
                    // enclosing function, which also clones the
                    // parameter-marker set, so a captured outer formal
                    // must not be reclassified. A declared formal whose
                    // marker is gone was reassigned in the body (R
                    // rebinds rather than narrows), so it degrades to
                    // `local` at its reassignment site.
                    let is_formal_here = info.params.contains_key(name.as_str());
                    let is_param = is_formal_here && record.scope.parameter_bindings.contains(name);
                    let local_span = info.locals.get(name.as_str()).copied();
                    let (kind, definition_span) = if is_param {
                        (
                            "param",
                            info.params.get(name.as_str()).copied().or(local_span),
                        )
                    } else if let Some(span) = local_span {
                        ("local", Some(span))
                    } else if record.kind == ry_checker::ScopeRecordKind::Function {
                        ("closed-over", None)
                    } else {
                        ("imported", None)
                    };
                    // A closed-over binding has no site in this scope;
                    // point at the definition in the nearest enclosing
                    // scope that binds the name, when one was recorded.
                    let definition_span = definition_span
                        .or_else(|| enclosing_binding_span(&infos, &chains[index], name.as_str()));
                    (name, ty, kind, definition_span, is_formal_here)
                })
                .filter(|(_, _, _, definition_span, is_formal_here)| {
                    // Visibility at the selecting positions: formals bind
                    // at call entry (always visible); a local assigned
                    // after every selecting position is not yet in scope
                    // there; a binding with no resolvable site is kept
                    // (it predates the body).
                    let Some(offsets) = selecting_offsets.get(&index) else {
                        return true;
                    };
                    if *is_formal_here {
                        return true;
                    }
                    let Some(span) = definition_span else {
                        return true;
                    };
                    offsets.iter().any(|offset| span.start <= *offset)
                })
                .map(|(name, ty, kind, definition_span, _)| BindingDump {
                    name: name.clone(),
                    kind,
                    type_: dump_type_string(ty),
                    start: definition_span.map(|span| offset_to_line_char_col(source, span.start)),
                })
                .collect();
            bindings.sort_by(|a, b| a.name.cmp(&b.name));

            ScopeDump {
                kind: match record.kind {
                    ry_checker::ScopeRecordKind::Function => "function",
                    ry_checker::ScopeRecordKind::Top => "top",
                },
                name: record.name.clone(),
                start: offset_to_line_char_col(source, record.span.start),
                end: offset_to_line_char_col(source, record.span.end),
                bindings,
            }
        })
        .collect();

    FileDump {
        path: path.to_string(),
        scopes,
    }
}

/// Sorted (nearest-first) enclosing-scope index chain for every scope:
/// each entry lists the other scopes whose span contains it, ordered by
/// smallest extent first. Computed once per file so every binding lookup
/// in a scope reuses the same chain.
fn enclosing_scope_chains(infos: &[ScopeInfo]) -> Vec<Vec<usize>> {
    (0..infos.len())
        .map(|index| {
            let inner = infos[index].record.span;
            let mut chain: Vec<usize> = (0..infos.len())
                .filter(|other| {
                    *other != index
                        && infos[*other].record.span.start <= inner.start
                        && inner.end <= infos[*other].record.span.end
                })
                .collect();
            // Nearest first: smallest enclosing extent.
            chain.sort_by_key(|other| {
                infos[*other].record.span.end - infos[*other].record.span.start
            });
            chain
        })
        .collect()
}

/// Definition site of `name` in the nearest recorded scope enclosing the
/// scope that owns `chain`, walking outward. Used for closed-over
/// bindings, whose only site in this file lives in an outer scope.
fn enclosing_binding_span(
    infos: &[ScopeInfo],
    chain: &[usize],
    name: &str,
) -> Option<ry_core::Span> {
    for index in chain {
        let info = &infos[*index];
        if let Some(span) = info.params.get(name).copied() {
            return Some(span);
        }
        if let Some(span) = info.locals.get(name).copied() {
            return Some(span);
        }
    }
    None
}

fn run_dump_types(
    files: Vec<PathBuf>,
    project_root: Option<PathBuf>,
    format: &str,
    positions: Vec<(usize, usize)>,
) -> Result<ExitCode> {
    if format != "json" {
        return Err(miette::miette!(
            "unknown --format `{}`; expected one of: json",
            format
        ));
    }

    // Config discovery mirrors `ry check`: anchored at the first input,
    // missing config is fine, malformed config aborts. The config's
    // directory is kept as `config_root` — it anchors the `exclude`
    // patterns below and is the resolution-root fallback for non-package
    // files, exactly as in `run_check`.
    let search_start = files.first().cloned().unwrap_or_else(|| PathBuf::from("."));
    let (config_root, cfg) = match config::Config::discover(&search_start) {
        Ok(Some((path, cfg))) => (path.parent().map(PathBuf::from), cfg),
        Ok(None) => (None, config::Config::defaults()),
        Err(e) => {
            eprintln!("ry: {}", e);
            return Ok(ExitCode::FAILURE);
        }
    };

    let mut all_paths = Vec::new();
    for root in &files {
        if !root.exists() {
            eprintln!("ry: {}: no such file or directory", root.display());
            return Ok(ExitCode::FAILURE);
        }
        let result = ry_workspace::discover_r_files(
            root,
            config_root.as_deref(),
            &cfg,
            cfg.check_test_fixtures,
        );
        all_paths.extend(result.files);
        report_truncation(&result.truncated, root);
    }
    sort_and_deduplicate_paths(&mut all_paths);

    // Parallel parsing with the same thread-local parser pool as
    // `ry check` (tree-sitter parsers are not Send).
    thread_local! {
        static DUMP_PARSER: std::cell::RefCell<Option<ry_core::RParser>> =
            const { std::cell::RefCell::new(None) };
    }
    use rayon::prelude::*;
    // Ok(Some(..)) = parsed; Ok(None) = unparseable (warned and skipped;
    // the dump still covers every parseable file); Err(..) = unreadable
    // (fatal, reported after the join).
    type ParseOutcome = Result<Option<(String, String, ry_core::SourceFile)>, String>;
    let results: Vec<ParseOutcome> = all_paths
        .par_iter()
        .map(|path| {
            let src = match read_r_source(path) {
                Ok(src) => src,
                Err(e) => return Err(format!("{}: {}", path.display(), e)),
            };
            let path_str = path.to_string_lossy().to_string();
            let file = DUMP_PARSER.with(|cell| {
                let mut slot = cell.borrow_mut();
                let parser = slot.get_or_insert_with(|| {
                    ry_core::RParser::new().expect("parser init (thread-local)")
                });
                parser.parse(&path_str, &src)
            });
            match file {
                Ok(file) => Ok(Some((path_str, src, file))),
                Err(e) => {
                    eprintln!(
                        "ry: {}: parse error: {e}; file omitted from dump",
                        path.display()
                    );
                    Ok(None)
                }
            }
        })
        .collect();

    let mut parsed: Vec<(String, String, ry_core::SourceFile)> = Vec::new();
    for result in results {
        match result {
            Err(read_error) => {
                eprintln!("ry: {read_error}");
                return Ok(ExitCode::FAILURE);
            }
            Ok(Some(entry)) => parsed.push(entry),
            Ok(None) => {}
        }
    }

    let user_stubs = load_user_stubs(&cfg.typeshed);

    // Same per-package grouping as `ry check`: each DESCRIPTION root is
    // its own library namespace. Non-package scripts share one group
    // rooted at --project-root, else the config root (the directory
    // owning the discovered ry.toml), else the working directory —
    // `run_check_once`'s fallback chain with --project-root overriding.
    let mut groups: std::collections::BTreeMap<Option<PathBuf>, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, (path, _, _)) in parsed.iter().enumerate() {
        groups
            .entry(enclosing_package_root(std::path::Path::new(path)))
            .or_default()
            .push(index);
    }

    let mut records_by_path: HashMap<String, Vec<ry_checker::ScopeRecord>> = HashMap::new();
    for (group_root, indices) in &groups {
        let resolution_root = group_root
            .clone()
            .or_else(|| project_root.clone())
            .or_else(|| config_root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let package_scope = ry_workspace::resolve_workspace_context(
            &resolution_root,
            &cfg,
            ry_workspace::ResolutionEnvironment {
                files: indices.iter().map(|index| &parsed[*index].2).collect(),
                user_stubs: &user_stubs,
            },
        )
        .map_err(|error| miette::miette!(error))?;
        let analysis_files: Vec<_> = indices
            .iter()
            .map(|index| {
                let (path, _, file) = &parsed[*index];
                (path.clone(), 0, std::sync::Arc::new(file.clone()))
            })
            .collect();
        let check_input = check::CheckInput {
            files: analysis_files,
            user_stubs: Arc::clone(&user_stubs),
            workspace: Some(ry_workspace::WorkspaceContext {
                attached_packages: package_scope.attached_packages,
                bare_bindings: package_scope.bare_bindings,
                external_bindings: package_scope.external_bindings,
                imported_bindings: package_scope.imported_bindings,
                s3_methods: package_scope.s3_methods,
                load_bindings: package_scope.load_bindings,
                degraded_scopes: Vec::new(),
            }),
        };
        // `ry check` prints one summary line per degraded scope; keep the
        // note on stderr here too so a dump over the same project reports
        // the same precision loss without polluting the JSON on stdout.
        for (path, reason) in &package_scope.degraded_scopes {
            eprintln!(
                "ry: {}: degraded scope ({reason}); serialized data file(s) over the byte cap fell back to file stems",
                path.display()
            );
        }
        for (path, records) in check::check_project_with_scope_capture(check_input) {
            records_by_path.insert(path, records);
        }
    }

    let dump = TypesDump {
        files: parsed
            .iter()
            .map(|(path, _src, file)| {
                let records = records_by_path.remove(path).unwrap_or_default();
                assemble_file_dump(path, file, records, &positions)
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&dump).into_diagnostic()?);
    // Diagnostics (if any) never affect the dump's exit code.
    Ok(ExitCode::SUCCESS)
}

fn sort_and_deduplicate_diagnostics(diagnostics: &mut Vec<ry_checker::Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        b.confidence.cmp(&a.confidence).then(
            a.path
                .cmp(&b.path)
                .then(a.span.line.cmp(&b.span.line))
                .then(a.span.col.cmp(&b.span.col))
                .then(a.span.start.cmp(&b.span.start))
                .then(a.span.end.cmp(&b.span.end))
                .then(a.code.cmp(b.code))
                .then(a.confidence.cmp(&b.confidence))
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
    println!("vendored snapshot:");
    for line in ry_typeshed::SOURCE.trim().lines() {
        println!("  {line}");
    }
    println!("embedded packages:");
    println!("  base");
    for package in ry_typeshed::known_packages() {
        println!("  {package}");
    }

    let cwd = std::env::current_dir().into_diagnostic()?;
    let dirs = match config::Config::discover(&cwd) {
        Ok(Some((_path, config))) => config.typeshed,
        Ok(None) => Vec::new(),
        Err(error) => {
            eprintln!("ry: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };
    println!("user stub directories:");
    if dirs.is_empty() {
        println!("  (none)");
    }
    for dir in dirs {
        println!("  {}", dir.display());
        match ry_typeshed::load_stub_dir_with_warnings(&dir) {
            Ok((stubs, warnings)) => {
                for package in stubs.keys() {
                    println!("    {package}");
                }
                for warning in warnings {
                    eprintln!("ry: warning: {warning}");
                }
            }
            Err(error) => eprintln!("ry: warning: {error}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_typeshed_validate(dirs: &[PathBuf], quiet: bool) -> Result<ExitCode> {
    let report = ry_typeshed::validate_stub_dirs(dirs);
    let errors = report.error_count();
    let warnings = report.warning_count();
    if !quiet {
        for problem in &report.problems {
            let level = match problem.level {
                ry_typeshed::ValidationLevel::Error => "error",
                ry_typeshed::ValidationLevel::Warning => "warning",
            };
            eprintln!("{}: {level}: {}", problem.path.display(), problem.message);
        }
    }
    println!(
        "Validated {} stub files: {errors} errors, {warnings} warnings.",
        report.files
    );
    Ok(if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
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

/// Read an R source file, accepting both UTF-8 and Latin-1 encodings.
///
/// R accepts Latin-1 source files, so retry an invalid UTF-8 decode by mapping
/// every input byte directly to the corresponding Unicode code point.
fn read_r_source(path: &std::path::Path) -> std::io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(source),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            std::fs::read(path).map(|bytes| bytes.into_iter().map(char::from).collect())
        }
        Err(error) => Err(error),
    }
}

fn enclosing_package_root(path: &std::path::Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    start
        .ancestors()
        .find(|ancestor| ancestor.join("DESCRIPTION").is_file())
        .map(std::path::Path::to_path_buf)
}

/// Test-compatible wrapper around the shared bounded discovery module.
/// Production code calls [`ry_workspace::discover_r_files`] directly with
/// the effective folder config so CLI and LSP use identical discovery
/// rules (P36-W7 / issue #48).
#[cfg(test)]
fn collect_r_files(path: &std::path::Path, out: &mut Vec<PathBuf>, check_test_fixtures: bool) {
    let result = ry_workspace::discover_r_files(
        path,
        None,
        &ry_config::Config::default(),
        check_test_fixtures,
    );
    out.extend(result.files);
}

/// Surface a discovery cap hit to the user (P36-W7). A cap hit is never
/// silent: the CLI prints one warning per root when any limit is reached.
fn report_truncation(report: &ry_workspace::TruncationReport, root: &std::path::Path) {
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
            "ry: warning: {} ({} bytes) exceeds the per-file size cap              (index.max-file-bytes) and was not discovered",
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

fn sort_and_deduplicate_paths(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}

#[cfg(test)]
mod tests {
    use super::config::{
        Baseline, BaselineEntry, load_baseline, subtract_baseline, write_baseline_file,
    };
    use super::{
        ColorChoice, collect_r_files, demote_non_source_paths, run_check_once,
        sort_and_deduplicate_diagnostics,
    };
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
    fn rbuildignore_trailing_dollar_respects_escape_parity() {
        assert!(
            ry_workspace::rbuildignore_pattern("^file$")
                .unwrap()
                .matches("file")
        );
        assert!(
            !ry_workspace::rbuildignore_pattern("^file$")
                .unwrap()
                .matches("filex")
        );
        assert!(
            ry_workspace::rbuildignore_pattern(r"^file\$")
                .unwrap()
                .matches("file$")
        );
        assert!(
            ry_workspace::rbuildignore_pattern(r"^file\\$")
                .unwrap()
                .matches(r"file\")
        );
        assert!(
            !ry_workspace::rbuildignore_pattern(r"^file\\$")
                .unwrap()
                .matches(r"file\x")
        );
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
    fn baseline_round_trip_suppresses_existing_but_not_new_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("baseline.json");
        let existing = diag("a.R", 1, 0, "RY010");
        write_baseline_file(&path, std::slice::from_ref(&existing), Some(temp.path())).unwrap();
        let baseline = load_baseline(&path).unwrap();
        let mut diagnostics = vec![existing, diag("a.R", 2, 0, "RY030")];
        subtract_baseline(&mut diagnostics, &baseline, Some(temp.path()));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "RY030");
    }

    #[test]
    fn baseline_counts_absorb_only_the_recorded_occurrences() {
        let baseline = Baseline {
            version: 1,
            entries: vec![BaselineEntry {
                path: "a.R".to_string(),
                code: "RY010".to_string(),
                message: "same message".to_string(),
                count: 2,
            }],
        };
        let mut diagnostics = vec![
            diag("a.R", 1, 0, "RY010"),
            diag("a.R", 2, 0, "RY010"),
            diag("a.R", 3, 0, "RY010"),
        ];
        subtract_baseline(&mut diagnostics, &baseline, None);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn high_minimum_hides_medium_confidence() {
        let mut diagnostics = vec![diag("a.R", 1, 0, "RY010"), diag("a.R", 2, 0, "RY030")];
        diagnostics.retain(|diagnostic| diagnostic.confidence >= ry_checker::Confidence::High);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "RY030");
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

        let mut paths = Vec::new();
        collect_r_files(temp.path(), &mut paths, false);
        paths.sort();
        let result = run_check_once(
            &paths,
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(temp.path()),
            ry_checker::Confidence::Low,
        )
        .unwrap();
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010"
                && diagnostic.path.contains("second")
                && diagnostic.message.contains("only_in_first")
        }));
    }

    #[test]
    fn package_scan_skips_test_fixtures_but_keeps_executable_test_code() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        for directory in [
            "R",
            "tests/testthat",
            "tests/testthat/fixtures",
            "tests/testthat/_snaps",
            "tests/manual",
            "revdep/other/R",
            "src/ratfor",
        ] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        for file in [
            "R/package.R",
            "tests/testthat.R",
            "tests/testthat/test-package.R",
            "tests/testthat/helper-package.R",
            "tests/testthat/setup-package.R",
            "tests/testthat/teardown-package.R",
            "tests/testthat/fixtures/input.R",
            "tests/testthat/data.R",
            "tests/testthat/_snaps/output.R",
            "tests/manual/example.R",
            "revdep/other/R/other.R",
            "src/ratfor/program.r",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);
        paths.sort();

        assert_eq!(
            paths,
            vec![
                root.join("R/package.R"),
                root.join("tests/testthat/helper-package.R"),
                root.join("tests/testthat/setup-package.R"),
                root.join("tests/testthat/teardown-package.R"),
                root.join("tests/testthat/test-package.R"),
                root.join("tests/testthat.R"),
            ]
        );
    }

    #[test]
    fn package_scan_can_opt_into_test_fixtures() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir_all(root.join("tests/testthat/fixtures")).unwrap();
        let fixture = root.join("tests/testthat/fixtures/input.R");
        std::fs::write(&fixture, "missing_name\n").unwrap();

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, true);

        assert_eq!(paths, vec![fixture]);
    }

    #[test]
    fn package_scan_keeps_inst_sources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir_all(root.join("inst/resources")).unwrap();
        let installed = root.join("inst/resources/activate.R");
        std::fs::write(&installed, "missing_name\n").unwrap();

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);

        assert_eq!(paths, vec![installed]);
    }

    #[test]
    fn package_scan_skips_vendored_renv_bootstrap() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir_all(root.join("R")).unwrap();
        std::fs::create_dir_all(root.join("renv")).unwrap();
        let source = root.join("R/package.R");
        std::fs::write(&source, "value <- 1L\n").unwrap();
        std::fs::write(root.join("renv/activate.R"), "bootstrap_missing\n").unwrap();

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);

        assert_eq!(paths, vec![source]);
    }

    #[test]
    fn collection_skips_rcheck_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("example.Rcheck/R")).unwrap();
        let source = root.join("source.R");
        std::fs::write(&source, "source_missing\n").unwrap();
        std::fs::write(root.join("example.Rcheck/R/copied.R"), "copied_missing\n").unwrap();

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);

        assert_eq!(paths, vec![source.clone()]);

        let result = run_check_once(
            &paths,
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(root),
            ry_checker::Confidence::Low,
        )
        .unwrap();
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
    fn collection_includes_all_supported_r_source_extensions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for extension in ["R", "r", "S", "s", "q"] {
            std::fs::write(root.join(format!("source.{extension}")), "value <- 1L\n").unwrap();
        }
        std::fs::write(root.join("source.txt"), "not R\n").unwrap();

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);
        paths.sort();

        let mut expected = ["R", "r", "S", "s", "q"]
            .map(|extension| root.join(format!("source.{extension}")))
            .into_iter()
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(paths, expected);
    }

    #[test]
    fn explicitly_selected_file_is_not_package_excluded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        let file = root.join("src/ratfor.r");
        std::fs::write(&file, "").unwrap();

        let mut paths = Vec::new();
        collect_r_files(&file, &mut paths, false);

        assert_eq!(paths, vec![file]);
    }

    #[test]
    fn explicitly_selected_q_file_is_collected() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("source.q");
        std::fs::write(&file, "value <- 1L\n").unwrap();

        let mut paths = Vec::new();
        collect_r_files(&file, &mut paths, false);

        assert_eq!(paths, vec![file]);
    }

    #[test]
    fn package_scan_honors_rbuildignore_except_r_and_tests() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::write(root.join(".Rbuildignore"), "^ignored\\.R$\n^R/\n^tests/\n").unwrap();
        std::fs::create_dir_all(root.join("R")).unwrap();
        std::fs::create_dir_all(root.join("tests/testthat")).unwrap();
        for file in [
            "ignored.R",
            "kept.R",
            "R/package.R",
            "tests/testthat/test-package.R",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);
        paths.sort();
        assert_eq!(
            paths,
            vec![
                root.join("R/package.R"),
                root.join("kept.R"),
                root.join("tests/testthat/test-package.R"),
            ]
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

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);
        paths.sort();
        let result = run_check_once(
            &paths,
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(root),
            ry_checker::Confidence::Low,
        )
        .unwrap();
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

        let mut paths = Vec::new();
        collect_r_files(root, &mut paths, false);
        paths.sort();
        let result = run_check_once(
            &paths,
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(root),
            ry_checker::Confidence::Low,
        )
        .unwrap();
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
        let result = run_check_once(
            &[file],
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(temp.path()),
            ry_checker::Confidence::Low,
        )
        .unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "RY097");
        assert_eq!(result.diagnostics[0].severity, Severity::Info);
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

        let result = run_check_once(
            &[file],
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(temp.path()),
            ry_checker::Confidence::Low,
        )
        .unwrap();
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

        let result = run_check_once(
            &[file],
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(temp.path()),
            ry_checker::Confidence::Low,
        )
        .unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "RY097");
    }

    #[test]
    fn latin1_source_comment_does_not_skip_checking() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("latin1.R");
        std::fs::write(&file, b"# Caf\xe9\nmissing_name\n").unwrap();

        let result = run_check_once(
            &[file],
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(temp.path()),
            ry_checker::Confidence::Low,
        )
        .unwrap();

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

        let result = run_check_once(
            &[file],
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(temp.path()),
            ry_checker::Confidence::Low,
        )
        .unwrap();
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

        let result = run_check_once(
            &[file],
            &ry_checker::SeverityFilter::default(),
            OutputFormat::Json,
            &ry_config::Config::defaults(),
            std::sync::Arc::new(std::collections::BTreeMap::new()),
            false,
            None,
            Some(temp.path()),
            ry_checker::Confidence::Low,
        )
        .unwrap();
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

    // ---- dump-types helpers ----

    #[test]
    fn dump_position_parser_accepts_pairs_and_rejects_garbage() {
        use super::parse_dump_position;
        assert_eq!(parse_dump_position("3:14").unwrap(), (3, 14));
        assert_eq!(parse_dump_position(" 12 : 1 ").unwrap(), (12, 1));
        assert!(parse_dump_position("3").is_err());
        assert!(parse_dump_position("a:b").is_err());
        assert!(parse_dump_position("0:1").is_err(), "rows are 1-based");
        assert!(parse_dump_position("1:0").is_err(), "cols are 1-based");
    }

    #[test]
    fn dump_position_offsets_round_trip_and_clamp() {
        use super::{line_char_col_to_offset, offset_to_line_char_col};
        // Multi-byte characters: columns must count characters, not bytes.
        let src = "a <- 1L\n#\u{e9} <- 2L\nlast <- 3L\n";
        assert_eq!(offset_to_line_char_col(src, 0), (1, 1));
        // Start of line 3.
        assert_eq!(
            offset_to_line_char_col(src, src.find("last").unwrap()),
            (3, 1)
        );
        // The identifier on line 2 starts after `#`, a multi-byte char.
        let ident = src.find("<- 2L").unwrap();
        let (row, col) = offset_to_line_char_col(src, ident);
        assert_eq!((row, col), (2, 4));
        assert_eq!(
            line_char_col_to_offset(src, row, col),
            Some(ident),
            "round trip"
        );
        // Row past the end matches nothing; column past the line end
        // clamps to the line end.
        assert_eq!(line_char_col_to_offset(src, 99, 1), None);
        assert_eq!(
            line_char_col_to_offset(src, 1, 500),
            Some(src.find('\n').unwrap())
        );
    }

    #[test]
    fn dump_type_string_renders_unknown_and_display_forms() {
        use super::dump_type_string;
        assert_eq!(dump_type_string(&ry_core::RType::unknown()), "unknown");
        let integer =
            ry_core::RType::new(ry_core::types::Mode::Integer, ry_core::types::Length::One);
        assert_eq!(dump_type_string(&integer), "integer<len=1>");
    }

    #[test]
    fn dump_local_binding_scan_skips_nested_function_bodies() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(
                "a.R",
                "outer <- function(x) {\n  keep <- 1L\n  inner <- function(y) { skip <- 2L }\n  if (x) keep2 <- 3L\n  for (i in 1:3) keep3 <- i\n  keep\n}\n",
            )
            .unwrap();
        // The function body's own locals, extracted from the statement
        // tree: nested `skip` belongs to the inner scope, not this one.
        let ry_core::ast::Stmt::Assign { value, .. } = &file.stmts[0] else {
            panic!("expected assignment");
        };
        let ry_core::ast::Expr::Function { body, .. } = value else {
            panic!("expected function literal");
        };
        let mut locals = std::collections::HashMap::new();
        super::collect_local_bindings(body, &mut locals);
        let mut names: Vec<_> = locals.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["i", "inner", "keep", "keep2", "keep3"]);
    }
}
