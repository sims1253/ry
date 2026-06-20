use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser as ClapParser, Subcommand};
use miette::{IntoDiagnostic, Result};

#[derive(Debug, ClapParser)]
#[command(
    name = "ry",
    version,
    about = "An extremely fast type checker for R",
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
        /// Output format. One of: full, concise, json.
        #[arg(long, value_name = "FORMAT", default_value = "concise")]
        output_format: String,
        /// Control when colored output is used.
        #[arg(long, value_name = "WHEN")]
        color: Option<String>,
    },
    /// Start the language server (not yet implemented).
    Server,
    /// Display ry's version.
    Version {
        /// Output format for version info.
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: String,
    },
    /// Explain a rule (or all rules).
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
    let cli = Cli::parse();
    init_tracing(cli.verbose, cli.quiet);

    let cmd = match cli.cmd {
        Some(c) => c,
        None => Cmd::Check {
            paths: Vec::new(),
            error: Vec::new(),
            warn: Vec::new(),
            ignore: Vec::new(),
            error_on_warning: false,
            exit_zero: false,
            output_format: "concise".to_string(),
            color: None,
        },
    };

    match cmd {
        Cmd::Check {
            paths,
            error,
            warn,
            ignore,
            error_on_warning,
            exit_zero,
            output_format,
            color: _,
        } => run_check(
            paths,
            error,
            warn,
            ignore,
            error_on_warning,
            exit_zero,
            &output_format,
        ),
        Cmd::Server => {
            eprintln!("ry: `server` is not implemented yet (planned for LSP integration)");
            Ok(ExitCode::FAILURE)
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

fn build_filter(error: &[String], warn: &[String], ignore: &[String]) -> ry_checker::SeverityFilter {
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

#[allow(clippy::too_many_arguments)]
fn run_check(
    paths: Vec<PathBuf>,
    error: Vec<String>,
    warn: Vec<String>,
    ignore: Vec<String>,
    error_on_warning: bool,
    exit_zero: bool,
    output_format: &str,
) -> Result<ExitCode> {
    let format = ry_checker::format::OutputFormat::parse(output_format).ok_or_else(|| {
        miette::miette!(
            "unknown --output-format `{}`; expected one of: full, concise, json",
            output_format
        )
    })?;
    let filter = build_filter(&error, &warn, &ignore);

    // ty defaults to the project root when no paths are given.
    let mut all_paths = Vec::new();
    let search_roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths
    };
    for root in &search_roots {
        collect_r_files(root, &mut all_paths);
    }
    if all_paths.is_empty() {
        eprintln!("ry: no .R / .r files found in {:?}", search_roots);
        return Ok(ExitCode::FAILURE);
    }

    let mut all_diagnostics: Vec<ry_checker::Diagnostic> = Vec::new();
    let mut srcs: HashMap<String, String> = HashMap::new();
    let mut parse_errors = 0usize;

    for path in &all_paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("ry: {}: {}", path.display(), e);
                parse_errors += 1;
                continue;
            }
        };
        let path_str = path.to_string_lossy().to_string();
        let mut parser = ry_core::RParser::new().map_err(|e| miette::miette!(e))?;
        let file = match parser.parse(&path_str, &src) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("ry: {}: parse error: {}", path.display(), e);
                parse_errors += 1;
                continue;
            }
        };
        let mut checker = ry_checker::Checker::new(&path_str);
        checker.check(&file);
        checker.apply_filter(&filter);
        all_diagnostics.extend(checker.take_diagnostics());
        srcs.insert(path_str, src);
    }

    all_diagnostics.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.line.cmp(&b.span.line))
            .then(a.span.col.cmp(&b.span.col))
    });

    let rendered = ry_checker::format::render(&all_diagnostics, format, &srcs);
    if !rendered.is_empty() {
        // JSON (machine-readable) goes to stdout; human formats go to
        // stderr so stdout stays clean for piping.
        match format {
            ry_checker::format::OutputFormat::Json => print!("{}", rendered),
            _ => eprint!("{}", rendered),
        }
    }

    let errors = all_diagnostics
        .iter()
        .filter(|d| d.severity == ry_checker::Severity::Error)
        .count();
    let warnings = all_diagnostics
        .iter()
        .filter(|d| d.severity == ry_checker::Severity::Warning)
        .count();

    eprintln!(
        "ry: checked {} file(s), {} error(s), {} warning(s)",
        all_paths.len(),
        errors,
        warnings
    );

    let failed = errors > 0
        || parse_errors > 0
        || (error_on_warning && warnings > 0);
    if exit_zero {
        Ok(ExitCode::SUCCESS)
    } else if failed {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn print_version(format: &str) {
    let v = env!("CARGO_PKG_VERSION");
    match format {
        "json" => println!(
            "{{\"name\":\"ry\",\"version\":\"{}\"}}",
            v
        ),
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
                        r.code, r.name, r.default_severity.as_str(), r.summary
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
