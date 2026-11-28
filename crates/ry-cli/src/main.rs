mod check;
mod dump;
mod facts;
mod facts_declarations;
mod facts_types;
mod output;
mod pipeline;
mod reduce;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, FromArgMatches, Parser as ClapParser, Subcommand, ValueEnum};
use miette::{IntoDiagnostic, Result};

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

/// Arguments of `ry check`. A bare `ry` runs `ry check`.
#[derive(Debug, Args)]
struct CheckArgs {
    /// Files or directories to check. Use `-` with --stdin-filename PATH
    /// for one in-memory source. Defaults to the current working directory.
    paths: Vec<PathBuf>,
    /// Logical source path for an explicit `-` stdin operand. Used for
    /// config discovery, package context, and diagnostic paths.
    #[arg(long, value_name = "PATH")]
    stdin_filename: Option<PathBuf>,
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
    /// Enable preview rules: newer rules that may still change. Their
    /// diagnostics are marked as preview. Same as `preview = true`.
    #[arg(long)]
    preview: bool,
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
    /// Explain included files and skipped paths on stderr before checking.
    #[arg(long)]
    explain_files: bool,
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

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Check a project (or files) for type errors.
    Check(CheckArgs),
    /// Dump inferred scope types as JSON.
    ///
    /// Write lexical scope bindings and their inferred types to stdout.
    /// Type strings keep the historical compact format. Editor hints use a
    /// separate bounded summary and expanded details.
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
        /// Include adopted source annotation records using schema version 3.
        #[arg(long)]
        annotations: bool,
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
    /// Shrink an R file while `ry check` still reports one diagnostic.
    ///
    /// Writes the smallest file found to stdout. Each candidate is checked
    /// in place of FILE, so it keeps FILE's project and configuration; FILE
    /// itself is not changed. A candidate must keep a diagnostic with the
    /// same code and message as the target.
    Reduce {
        /// The R file to shrink.
        file: PathBuf,
        /// Rule code of the diagnostic to keep, such as RY092.
        #[arg(long, value_name = "CODE")]
        code: String,
        /// Keep the diagnostic with this code on this line. Defaults to the
        /// first one in the file.
        #[arg(long, value_name = "N")]
        line: Option<usize>,
        /// Also require that each candidate runs without error under
        /// `Rscript --vanilla`, so a false positive stays one. This runs
        /// the candidates' R code.
        #[arg(long)]
        rscript: bool,
        /// Write the result to this file instead of stdout.
        #[arg(long, short = 'o', value_name = "PATH")]
        output: Option<PathBuf>,
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
    /// Show discovered R libraries and resolved package versions.
    Packages {
        /// Files or directories to inspect (defaults to the current directory).
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,
        /// Output format: text or json.
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: String,
    },
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
    // `ArgMatches` is kept so check's `flag_set` can tell a user-passed
    // flag from its clap default. Use `try_get_matches` so `--help` /
    // `--version` writes go through our broken-pipe-safe output seam (#920).
    let mut matches = match Cli::command().try_get_matches() {
        Ok(matches) => matches,
        Err(err) => return Ok(output::handle_clap_error(err)),
    };
    if matches.subcommand().is_none() {
        // Only global flags can precede a missing subcommand, and they
        // parse the same after `check`.
        let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
        if args.is_empty() {
            args.push("ry".into());
        }
        args.insert(1, "check".into());
        matches = match Cli::command().try_get_matches_from(args) {
            Ok(matches) => matches,
            Err(err) => return Ok(output::handle_clap_error(err)),
        };
    }
    let cli = Cli::from_arg_matches(&matches).expect("clap derive schema is self-consistent");
    let cmd = cli.cmd.expect("a bare `ry` re-parses as `ry check`");

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
            annotations,
        } => facts::run_dump_facts(files, project_root, &format, references, annotations),
        Cmd::Server { log_level } => {
            // stdout carries JSON-RPC, so tracing must write to stderr.
            let filter = tracing_subscriber::EnvFilter::try_new(&log_level)
                .map_err(|e| miette::miette!("invalid --log-level '{log_level}': {e}"))?;
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(filter)
                .try_init()
                .ok();
            // Only the server needs a tokio runtime.
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| miette::miette!("failed to start tokio runtime: {}", e))?;
            let code = rt.block_on(ry_lsp::run());
            // Tokio's stdin reader is a blocking read that cannot be
            // cancelled; waiting for it would keep the process alive until
            // the client closes stdin.
            rt.shutdown_background();
            Ok(code)
        }
        Cmd::Version { output_format } => {
            print_version(&output_format)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Reduce {
            file,
            code,
            line,
            rscript,
            output,
        } => reduce::run_reduce(reduce::ReduceArgs {
            file,
            code,
            line,
            rscript,
            output,
        }),
        Cmd::ExplainRule(args) => run_explain_rule(args.rule, &args.output_format),
        Cmd::Explain { command } => match command {
            ExplainCmd::Rule(args) => run_explain_rule(args.rule, &args.output_format),
            ExplainCmd::Typeshed => run_explain_typeshed(),
            ExplainCmd::Packages {
                paths,
                output_format,
            } => run_explain_packages(paths, &output_format),
        },
        Cmd::Typeshed {
            command: TypeshedCmd::Validate { dirs },
        } => run_typeshed_validate(&dirs, cli.quiet > 0),
        Cmd::GenerateShellCompletion { shell } => run_shell_completion(&shell),
    }
}

fn print_version(format: &str) -> Result<()> {
    let v = env!("CARGO_PKG_VERSION");
    match format {
        "json" => output::writeln_stdout(&format!("{{\"name\":\"ry\",\"version\":\"{v}\"}}")),
        _ => output::writeln_stdout(&format!("ry {v}")),
    }
}

fn run_explain_rule(rule: Option<String>, output_format: &str) -> Result<ExitCode> {
    let rules = ry_checker::rules::RULES;
    let matched: Vec<&'static ry_checker::rules::Rule> = match &rule {
        Some(name) => match ry_checker::rules::find(name) {
            Some(r) => vec![r],
            None => {
                crate::eprintln_safe!("ry: unknown rule `{}`", name);
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
                    let example = r.example();
                    let files: serde_json::Map<String, serde_json::Value> = example
                        .support
                        .iter()
                        .map(|(path, contents)| ((*path).to_string(), (*contents).into()))
                        .collect();
                    serde_json::json!({
                        "code": r.code,
                        "name": r.name,
                        "severity": r.default_severity.as_str(),
                        "status": r.status.as_str(),
                        "summary": r.summary,
                        "example": {
                            "extension": example.extension,
                            "flagged": example.flagged,
                            "fix": example.fix,
                            "fixed": example.fixed,
                            "files": files,
                        },
                    })
                })
                .collect();
            output::writeln_stdout(&serde_json::to_string_pretty(&json).into_diagnostic()?)?;
        }
        _ => {
            use std::fmt::Write as _;
            let mut buf = String::new();
            if matched.len() == 1 {
                let r = matched[0];
                let _ = writeln!(buf, "{} ({})", r.code, r.name);
                let _ = writeln!(buf, "Default severity: {}", r.default_severity);
                let _ = writeln!(buf, "Status: {}", status_line(r.status));
                let _ = writeln!(buf);
                let _ = writeln!(buf, "{}", r.summary);
                write_rule_example(&mut buf, &r.example());
            } else {
                let _ = writeln!(
                    buf,
                    "{:<8} {:<24} {:<10} {:<8} summary",
                    "code", "name", "severity", "status"
                );
                let _ = writeln!(buf, "{}", "-".repeat(78));
                for r in &matched {
                    let _ = writeln!(
                        buf,
                        "{:<8} {:<24} {:<10} {:<8} {}",
                        r.code,
                        r.name,
                        r.default_severity.as_str(),
                        r.status.as_str(),
                        r.summary
                    );
                }
            }
            output::write_stdout(&buf)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// A rule's status, with how it reaches output.
fn status_line(status: ry_checker::rules::RuleStatus) -> String {
    use ry_checker::rules::RuleStatus;
    let how = match status {
        RuleStatus::Stable => "on by default",
        RuleStatus::Preview => {
            "on with `preview = true`, `--preview`, `extend-select` or a severity override; may still change"
        }
        RuleStatus::OptIn => "off by default; enable with `extend-select` or a severity override",
    };
    format!("{} ({how})", status.as_str())
}

/// The worked example under a single rule's explanation.
fn write_rule_example(buf: &mut String, example: &ry_checker::rules::RuleExample) {
    use std::fmt::Write as _;
    fn indented(buf: &mut String, source: &str) {
        for line in source.lines() {
            if line.is_empty() {
                let _ = writeln!(buf);
            } else {
                let _ = writeln!(buf, "    {line}");
            }
        }
    }
    for (path, contents) in &example.support {
        let _ = writeln!(buf);
        let _ = writeln!(buf, "With `{path}`:");
        let _ = writeln!(buf);
        indented(buf, contents);
    }
    let _ = writeln!(buf);
    let _ = writeln!(buf, "Example (`example.{}`):", example.extension);
    let _ = writeln!(buf);
    indented(buf, example.flagged);
    let _ = writeln!(buf);
    let _ = writeln!(buf, "Fix: {}", example.fix);
    let _ = writeln!(buf);
    indented(buf, example.fixed);
}

fn run_explain_typeshed() -> Result<ExitCode> {
    use std::fmt::Write as _;
    let mut buf = String::new();
    let _ = writeln!(buf, "vendored snapshot:");
    for line in ry_typeshed::SOURCE.trim().lines() {
        let _ = writeln!(buf, "  {line}");
    }
    let _ = writeln!(buf, "embedded packages:");
    let _ = writeln!(buf, "  base");
    for package in ry_typeshed::known_packages() {
        let _ = writeln!(buf, "  {package}");
    }

    let cwd = std::env::current_dir().into_diagnostic()?;
    let dirs = match config::Config::discover(&cwd) {
        Ok(Some((_path, config))) => config.typeshed,
        Ok(None) => Vec::new(),
        Err(error) => {
            crate::eprintln_safe!("ry: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };
    let _ = writeln!(buf, "user stub directories:");
    if dirs.is_empty() {
        let _ = writeln!(buf, "  (none)");
    }
    for dir in dirs {
        let _ = writeln!(buf, "  {}", dir.display());
        match ry_typeshed::load_stub_dir_with_warnings(&dir) {
            Ok((stubs, warnings)) => {
                for package in stubs.keys() {
                    let _ = writeln!(buf, "    {package}");
                }
                for warning in warnings {
                    crate::eprintln_safe!("ry: warning: {warning}");
                }
            }
            Err(error) => crate::eprintln_safe!("ry: warning: {error}"),
        }
    }
    output::write_stdout(&buf)?;
    Ok(ExitCode::SUCCESS)
}

fn run_explain_packages(paths: Vec<PathBuf>, output_format: &str) -> Result<ExitCode> {
    if !matches!(output_format, "text" | "json") {
        crate::eprintln_safe!(
            "ry: unknown output format '{output_format}' (expected text or json)"
        );
        return Ok(ExitCode::FAILURE);
    }
    let cwd = std::env::current_dir().into_diagnostic()?;
    let search_roots = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths
    };
    let missing: Vec<&PathBuf> = search_roots.iter().filter(|root| !root.exists()).collect();
    if !missing.is_empty() {
        for root in missing {
            crate::eprintln_safe!("ry: {}: no such file or directory", root.display());
        }
        return Ok(ExitCode::FAILURE);
    }
    let search_start = search_roots
        .first()
        .map(|p| {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                cwd.join(p)
            };
            if abs.is_dir() {
                abs
            } else {
                abs.parent().unwrap_or(&cwd).to_path_buf()
            }
        })
        .unwrap_or_else(|| cwd.clone());
    let (config_root, cfg) = match config::Config::discover(&search_start) {
        Ok(Some((path, cfg))) => (path.parent().map(PathBuf::from), cfg),
        Ok(None) => (None, config::Config::default()),
        Err(error) => {
            crate::eprintln_safe!("ry: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };
    let mut user_stubs = std::collections::BTreeMap::new();
    for dir in &cfg.typeshed {
        if let Ok((loaded, _warnings)) = ry_typeshed::load_stub_dir_with_warnings(dir) {
            user_stubs.extend(loaded);
        }
    }
    let mut all_paths = Vec::new();
    for root in &search_roots {
        let discovered = ry_workspace::discover_r_files(
            root,
            config_root.as_deref(),
            &cfg,
            cfg.check_test_fixtures,
        );
        all_paths.extend(discovered.files);
    }
    let mut parsed = Vec::new();
    for path in &all_paths {
        if let Ok(file) = ry_workspace::parse_source_file(path, None) {
            parsed.push(std::sync::Arc::new(file));
        }
    }
    let mut packages: std::collections::BTreeSet<String> = cfg.packages.iter().cloned().collect();
    let mut group_resolved: std::collections::BTreeMap<
        (String, PathBuf),
        ry_workspace::ResolvedPackage,
    > = std::collections::BTreeMap::new();
    if let Ok(groups) =
        pipeline::resolve_groups(&parsed, &cfg, &user_stubs, &[config_root.as_deref()])
    {
        for group in groups {
            packages.extend(group.context.attached_packages);
            for imports in group.context.imported_bindings.into_values() {
                packages.extend(imports.into_values());
            }
            for pkg in group.context.resolved_packages.into_values() {
                group_resolved
                    .entry((pkg.name.clone(), pkg.library.clone()))
                    .or_insert(pkg);
            }
        }
    }
    let probe_paths = if all_paths.is_empty() {
        search_roots
    } else {
        all_paths
    };
    let unresolved_pkgs: Vec<String> = packages
        .into_iter()
        .filter(|name| !group_resolved.keys().any(|(resolved, _)| resolved == name))
        .collect();
    let mut env = ry_workspace::resolve_environment(&probe_paths, &unresolved_pkgs);
    for pkg in env.packages.drain(..) {
        group_resolved
            .entry((pkg.name.clone(), pkg.library.clone()))
            .or_insert(pkg);
    }
    env.packages = group_resolved.into_values().collect();

    let stub_kind = |name: &str| -> &'static str {
        if user_stubs.contains_key(name) {
            "user"
        } else if name == "base" || ry_typeshed::known_packages().any(|pkg| pkg == name) {
            "embedded"
        } else {
            "none"
        }
    };

    if output_format == "json" {
        let packages_json: Vec<serde_json::Value> = env
            .packages
            .iter()
            .map(|pkg| {
                serde_json::json!({
                    "name": pkg.name,
                    "version": pkg.version,
                    "library": pkg.library.display().to_string(),
                    "stub": stub_kind(&pkg.name),
                })
            })
            .collect();
        let payload = serde_json::json!({
            "active_r_version": env.active_r_version,
            "library_roots": env.library_roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "packages": packages_json,
        });
        output::writeln_stdout(&serde_json::to_string_pretty(&payload).into_diagnostic()?)?;
        return Ok(ExitCode::SUCCESS);
    }

    use std::fmt::Write as _;
    let mut buf = String::new();
    let _ = writeln!(
        buf,
        "active R version: {}",
        env.active_r_version.as_deref().unwrap_or("(unknown)")
    );
    let _ = writeln!(buf, "library roots:");
    if env.library_roots.is_empty() {
        let _ = writeln!(buf, "  (none)");
    } else {
        for root in &env.library_roots {
            let _ = writeln!(buf, "  {}", root.display());
        }
    }
    let _ = writeln!(buf, "resolved packages:");
    if env.packages.is_empty() {
        let _ = writeln!(buf, "  (none)");
    } else {
        for pkg in &env.packages {
            let version = pkg.version.as_deref().unwrap_or("?");
            let stub = stub_kind(&pkg.name);
            if stub == "none" {
                let _ = writeln!(
                    buf,
                    "  {} {} ({})",
                    pkg.name,
                    version,
                    pkg.library.display()
                );
            } else {
                let _ = writeln!(
                    buf,
                    "  {} {} ({}) [{} stub]",
                    pkg.name,
                    version,
                    pkg.library.display(),
                    stub
                );
            }
        }
    }
    output::write_stdout(&buf)?;
    Ok(ExitCode::SUCCESS)
}

fn run_typeshed_validate(dirs: &[PathBuf], quiet: bool) -> Result<ExitCode> {
    let report = ry_typeshed::validate_stub_dirs(dirs);
    let errors = report.error_count();
    if !quiet {
        for problem in &report.problems {
            crate::eprintln_safe!("{}: error: {}", problem.path.display(), problem.message);
        }
    }
    output::writeln_stdout(&format!(
        "Validated {} stub files: {errors} errors.",
        report.files
    ))?;
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
            crate::eprintln_safe!("ry: unknown shell `{}`", other);
            return Ok(ExitCode::FAILURE);
        }
    };
    let mut buf = Vec::new();
    clap_complete::generate(shell_kind, &mut cmd, "ry", &mut buf);
    output::write_stdout_bytes(&buf)?;
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
