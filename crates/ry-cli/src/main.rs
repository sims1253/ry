mod check;
mod dump;
mod facts;
mod facts_types;
mod pipeline;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, FromArgMatches, Parser as ClapParser, Subcommand, ValueEnum};
use miette::{IntoDiagnostic, Result};
use rayon::ThreadPoolBuilder;

use ry_config as config;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
enum ConfidenceChoice {
    #[default]
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
        if !format.is_human() {
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
    /// Increase verbosity. Use -v for info, -vv for trace.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// Decrease verbosity. Use -q for quiet, -qq for silent.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    quiet: u8,
}

/// Arguments of `ry check` — and of a bare `ry`, which runs `ry check`.
///
/// Flag defaults live in one place. Empty vecs, unset flags, and absent
/// options default through their types. The three scalar defaults come
/// from [`config::DEFAULT_OUTPUT_FORMAT`] and the enums' `Default` impls,
/// which the clap attributes read from too, so the derive and
/// [`CheckArgs::default`] cannot drift apart.
#[derive(Debug, Args)]
struct CheckArgs {
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
    #[arg(long, value_name = "FORMAT", default_value_t = config::DEFAULT_OUTPUT_FORMAT.to_string())]
    output_format: String,
    /// Control ANSI color in human-readable output.
    #[arg(long, value_enum, default_value_t = ColorChoice::default())]
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
    #[arg(long, value_enum, default_value_t = ConfidenceChoice::default())]
    min_confidence: ConfidenceChoice,
}

impl Default for CheckArgs {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            error: Vec::new(),
            warn: Vec::new(),
            ignore: Vec::new(),
            typeshed: Vec::new(),
            error_on_warning: false,
            exit_zero: false,
            output_format: config::DEFAULT_OUTPUT_FORMAT.to_string(),
            color: ColorChoice::default(),
            watch: false,
            statistics: false,
            write_baseline: None,
            baseline: None,
            min_confidence: ConfidenceChoice::default(),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Check a project (or files) for type errors.
    Check(CheckArgs),
    /// Dump inferred scope types as JSON.
    ///
    /// Write lexical scope bindings and their inferred types to stdout.
    /// Type strings use the same format as the language server's inline hints.
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
        #[arg(long = "position", value_name = "LINE:COL", value_parser = dump::parse_dump_position)]
        positions: Vec<(usize, usize)>,
    },
    /// Export versioned structured analysis facts as JSON.
    DumpFacts {
        /// R files or directories to analyze.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Analysis root for files outside an R package.
        #[arg(long, value_name = "DIR")]
        project_root: Option<PathBuf>,
        /// Output format. Only `json` is supported.
        #[arg(long, value_name = "FORMAT", default_value = "json")]
        format: String,
        /// Include conservative reference facts using schema version 2.
        #[arg(long)]
        references: bool,
    },
    /// Start the language server over stdio.
    ///
    /// Connect from an editor that supports the Language Server Protocol (LSP)
    /// to receive diagnostics and type hints for R files.
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
    /// Explain a rule (or all rules).
    #[command(visible_alias = "rule")]
    ExplainRule(ExplainRuleArgs),
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
    /// Generate shell completions.
    GenerateShellCompletion {
        /// Target shell.
        shell: String,
    },
}

/// Arguments shared by `ry explain-rule` (alias `ry rule`) and
/// `ry explain rule`: both spellings select the same rule and output
/// format, so their flags cannot drift apart.
#[derive(Debug, Args)]
struct ExplainRuleArgs {
    /// Rule code or name. Omit to list all rules.
    rule: Option<String>,
    /// Output format: text or json.
    #[arg(long, value_name = "FORMAT", default_value = "text")]
    output_format: String,
}

#[derive(Debug, Subcommand)]
enum ExplainCmd {
    /// Explain a rule (or all rules).
    Rule(ExplainRuleArgs),
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
    // Size rayon's global pool before any parallel work starts. The
    // check pipeline alternates parallel phases (parsing, diagnostic
    // emission) with serial ones (workspace resolution, fixpoint
    // refinement), so a worker per core spends the serial phases
    // spinning and waking: on a 24-core machine the default pool burns
    // ~4x the system time of a 12-worker pool for the same wall time.
    // Capping at 12 keeps throughput flat while cutting CPU and system
    // time on large machines; an explicit RAYON_NUM_THREADS still wins.
    if std::env::var_os("RAYON_NUM_THREADS").is_none() {
        let workers = std::thread::available_parallelism()
            .map_or(8, std::num::NonZeroUsize::get)
            .min(12);
        if let Err(error) = ThreadPoolBuilder::new().num_threads(workers).build_global() {
            eprintln!("ry: warning: could not size the thread pool: {error}");
        }
    }

    // `ArgMatches` is kept alongside the typed `Cli` so check's
    // `flag_set` can tell a user-passed flag from its clap default.
    //
    // clap derive's `from_arg_matches` is infallible for our schema
    // (every arg has a default or is optional); the unwrap is safe.
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("clap derive schema is self-consistent");

    // Tracing is initialized inside `check::run_check` AFTER config discovery
    // so a `verbose = N` in `ry.toml` can take effect. Non-check
    // subcommands do not emit tracing events, so they do not need an
    // earlier init.

    let cmd = match cli.cmd {
        Some(c) => c,
        None => Cmd::Check(CheckArgs::default()),
    };

    // Kept only for `check`: detecting explicit CLI overrides of scalar
    // fields the config file can also set.
    let check_matches = matches.subcommand_matches("check");

    match cmd {
        Cmd::Check(args) => check::run_check(args, cli.verbose, cli.quiet, check_matches),
        Cmd::DumpTypes {
            files,
            project_root,
            format,
            positions,
        } => dump::run_dump_types(files, project_root, &format, positions),
        Cmd::DumpFacts {
            files,
            project_root,
            format,
            references,
        } => facts::run_dump_facts(files, project_root, &format, references),
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
        Cmd::ExplainRule(args) => run_explain_rule(args.rule, &args.output_format),
        Cmd::Explain { command } => match command {
            ExplainCmd::Rule(args) => run_explain_rule(args.rule, &args.output_format),
            ExplainCmd::Typeshed => run_explain_typeshed(),
        },
        Cmd::Typeshed {
            command: TypeshedCmd::Validate { dirs },
        } => run_typeshed_validate(&dirs, cli.quiet > 0),
        Cmd::GenerateShellCompletion { shell } => run_shell_completion(&shell),
    }
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
    if !quiet {
        for problem in &report.problems {
            eprintln!("{}: error: {}", problem.path.display(), problem.message);
        }
    }
    println!("Validated {} stub files: {errors} errors.", report.files);
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

#[cfg(test)]
mod tests {
    use super::ColorChoice;
    use ry_checker::format::OutputFormat;

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
