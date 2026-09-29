//! Bounded, R-backed semantic generation for the condition-cardinality family.
//! Campaign inputs come only from `Case::source`; fixed negative tests also
//! exercise rejection of unrelated R failures.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ry_checker::Checker;
use ry_core::RParser;
use serde_json::{Value, json};

const GENERATOR_VERSION: &str = "condition-cardinality-v1";
const WALL_LIMIT: Duration = Duration::from_secs(10);
const OUTPUT_LIMIT: u64 = 16 * 1024;
const SUCCESS_MARKER: &str = "RY_GENERATOR_PREMISE_OK";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    If,
    While,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cardinality {
    Zero,
    OneTrue,
    OneFalse,
    Many,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Case {
    seed: u64,
    control: Control,
    cardinality: Cardinality,
    alias: bool,
    spaced: bool,
    alternate_literal: bool,
}

impl Case {
    // Stable bit layout is part of the versioned generator contract. The
    // first eight seeds cover both control forms and all four cardinalities.
    fn from_seed(seed: u64) -> Self {
        assert!(seed < 64, "generator v1 has exactly 64 distinct seeds");
        Self {
            seed,
            control: if seed & 1 == 0 {
                Control::If
            } else {
                Control::While
            },
            cardinality: match (seed >> 1) & 3 {
                0 => Cardinality::Zero,
                1 => Cardinality::OneTrue,
                2 => Cardinality::OneFalse,
                _ => Cardinality::Many,
            },
            alias: seed & 8 != 0,
            spaced: seed & 16 != 0,
            alternate_literal: seed & 32 != 0,
        }
    }

    fn encoded_seed(&self) -> u64 {
        let control = u64::from(self.control == Control::While);
        let cardinality = match self.cardinality {
            Cardinality::Zero => 0,
            Cardinality::OneTrue => 1,
            Cardinality::OneFalse => 2,
            Cardinality::Many => 3,
        };
        control
            | (cardinality << 1)
            | (u64::from(self.alias) << 3)
            | (u64::from(self.spaced) << 4)
            | (u64::from(self.alternate_literal) << 5)
    }

    fn literal(&self) -> &'static str {
        match (self.cardinality, self.alternate_literal) {
            (Cardinality::Zero, false) => "logical(0)",
            (Cardinality::Zero, true) => "logical(0L)",
            (Cardinality::OneTrue, false) => "TRUE",
            (Cardinality::OneTrue, true) => "c(TRUE)",
            (Cardinality::OneFalse, false) => "FALSE",
            (Cardinality::OneFalse, true) => "c(FALSE)",
            (Cardinality::Many, false) => "c(TRUE, FALSE)",
            (Cardinality::Many, true) => "c(TRUE, FALSE, TRUE)",
        }
    }

    fn source(&self) -> String {
        // The sole emitted binding is fresh, local to a new R environment,
        // assigned exactly once, and read exactly once. Literal expressions
        // have no side effects or dispatch. Spaces do not change tokens.
        let mut source = String::new();
        if self.alias {
            source.push_str("condition <- ");
            source.push_str(self.literal());
            source.push('\n');
        }
        let expr = if self.alias {
            "condition"
        } else {
            self.literal()
        };
        let space = if self.spaced { "  " } else { " " };
        match self.control {
            Control::If => source.push_str(&format!("if{space}({expr}){space}TRUE else FALSE\n")),
            Control::While => source.push_str(&format!("while{space}({expr}){space}break\nTRUE\n")),
        }
        source
    }

    fn premise(&self) -> &'static str {
        match self.cardinality {
            Cardinality::Zero => "R rejects a length-zero if/while condition",
            Cardinality::Many => "R rejects a length-greater-than-one if/while condition",
            Cardinality::OneTrue | Cardinality::OneFalse => {
                "R evaluates a scalar logical if/while condition without error"
            }
        }
    }

    fn expected_code(&self) -> Option<&'static str> {
        match (self.control, self.cardinality) {
            (_, Cardinality::Zero) | (Control::While, Cardinality::Many) => Some("RY001"),
            (Control::If, Cardinality::Many) => Some("RY002"),
            _ => None,
        }
    }

    fn condition_start(&self) -> usize {
        let source = self.source();
        let prefix = match self.control {
            Control::If => "if",
            Control::While => "while",
        };
        let control = source.find(prefix).expect("generated control");
        let open = source[control..].find('(').expect("generated paren") + control;
        source[open + 1..]
            .find(|c: char| !c.is_whitespace())
            .expect("generated condition")
            + open
            + 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Agreement,
    Disagreement(&'static str),
    Malformed,
    UnsupportedPremise,
    RuntimeOtherError,
    RUnavailable,
    TimedOut,
    Crashed,
    OutputLimit,
    HarnessError,
}

#[derive(Debug)]
struct RResult {
    class: Status,
    exit: Option<i32>,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
struct Evaluation {
    status: Status,
    r: RResult,
    diagnostics: Vec<Value>,
}

fn assertion_script(case: &Case) -> String {
    let expected = match case.cardinality {
        Cardinality::Zero => "zero",
        Cardinality::Many => "many",
        Cardinality::OneTrue if case.control == Control::If => "true",
        Cardinality::OneFalse if case.control == Control::If => "false",
        _ => "true", // either while form finishes with the trailing TRUE
    };
    format!(
        r#"result <- tryCatch({{
  parsed <- parse(file = "case.R")
  list(kind = "value", value = eval(parsed, envir = new.env(parent = baseenv())))
}}, error = function(e) list(kind = "error", message = conditionMessage(e)))
expected <- "{expected}"
ok <- switch(expected,
  zero = identical(result$kind, "error") && grepl("argument is of length zero", result$message, fixed = TRUE),
  many = identical(result$kind, "error") && grepl("the condition has length > 1", result$message, fixed = TRUE),
  true = identical(result$kind, "value") && identical(result$value, TRUE),
  false = identical(result$kind, "value") && identical(result$value, FALSE))
if (!isTRUE(ok)) {{
  cat("RY_GENERATOR_OTHER:", result$kind, if (!is.null(result$message)) result$message, "\n", file = stderr())
  quit(save = "no", status = 44L)
}}
cat("{SUCCESS_MARKER}\n")
"#
    )
}

#[cfg(unix)]
fn set_resource_limits(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Every generated program is tiny. CPU, virtual memory, output file and
    // descriptor ceilings bound a wedged or unexpectedly expansive R run.
    unsafe {
        command.pre_exec(|| {
            for (resource, soft, hard) in [
                (libc::RLIMIT_CPU, 5, 5),
                (libc::RLIMIT_AS, 1024 * 1024 * 1024, 1024 * 1024 * 1024),
                (libc::RLIMIT_FSIZE, OUTPUT_LIMIT, OUTPUT_LIMIT),
                (libc::RLIMIT_NOFILE, 1024, 1024),
                (libc::RLIMIT_CORE, 0, 0),
            ] {
                let limit = libc::rlimit {
                    rlim_cur: soft,
                    rlim_max: hard,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}

fn read_capped(path: &Path) -> Result<(String, bool), std::io::Error> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(OUTPUT_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    // RLIMIT_FSIZE may stop the writer at exactly the cap, before a
    // cap-plus-one read is possible. Treat that boundary as output failure.
    let too_large = bytes.len() as u64 >= OUTPUT_LIMIT;
    if too_large {
        bytes.truncate(OUTPUT_LIMIT as usize);
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), too_large))
}

fn run_r(
    case: &Case,
    source: &str,
    rscript: &Path,
    sandbox: bool,
    wall_limit: Duration,
) -> RResult {
    let result = || -> Result<RResult, std::io::Error> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("case.R"), source)?;
        fs::write(temp.path().join("assert.R"), assertion_script(case))?;
        let stdout_path = temp.path().join("stdout");
        let stderr_path = temp.path().join("stderr");
        let mut command = if sandbox {
            let mut command = Command::new("bwrap");
            command.args([
                "--unshare-net",
                "--die-with-parent",
                "--ro-bind",
                "/",
                "/",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--bind",
            ]);
            command.arg(temp.path()).arg(temp.path());
            command.arg("--chdir").arg(temp.path()).arg("--");
            command.arg(rscript);
            command
        } else {
            Command::new(rscript)
        };
        command
            .args(["--vanilla", "assert.R"])
            .current_dir(temp.path())
            .env("R_DEFAULT_PACKAGES", "base")
            .env("R_LIBS_USER", temp.path().join("library"))
            .env("TMPDIR", temp.path())
            .env("HOME", temp.path())
            .stdin(Stdio::null())
            .stdout(File::create(&stdout_path)?)
            .stderr(File::create(&stderr_path)?);
        #[cfg(unix)]
        set_resource_limits(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) if !sandbox && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RResult {
                    class: Status::RUnavailable,
                    exit: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                });
            }
            Err(error) => return Err(error),
        };
        let start = Instant::now();
        let exit = loop {
            match child.try_wait() {
                Ok(Some(exit)) => break exit,
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
            if start.elapsed() >= wall_limit {
                let _ = child.kill();
                let _ = child.wait();
                let (stdout, _) = read_capped(&stdout_path)?;
                let (stderr, _) = read_capped(&stderr_path)?;
                return Ok(RResult {
                    class: Status::TimedOut,
                    exit: None,
                    stdout,
                    stderr,
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let (stdout, stdout_large) = read_capped(&stdout_path)?;
        let (stderr, stderr_large) = read_capped(&stderr_path)?;
        let class = if stdout_large || stderr_large {
            Status::OutputLimit
        } else if exit.success() && stdout.trim() == SUCCESS_MARKER {
            Status::Agreement // R predicate proven; checker still pending.
        } else if exit.code() == Some(44) && stderr.contains("RY_GENERATOR_OTHER:") {
            Status::RuntimeOtherError
        } else if exit.code().is_none() {
            Status::Crashed
        } else {
            Status::HarnessError
        };
        Ok(RResult {
            class,
            exit: exit.code(),
            stdout,
            stderr,
        })
    }();
    result.unwrap_or_else(|error| RResult {
        class: Status::HarnessError,
        exit: None,
        stdout: String::new(),
        stderr: error.to_string(),
    })
}

fn evaluate_source(
    case: &Case,
    source: &str,
    rscript: &Path,
    sandbox: bool,
    inject_missing_ry002: bool,
) -> Evaluation {
    let mut parser = RParser::new().expect("parser initialization");
    let file = match parser.parse("case.R", source) {
        Ok(file) if file.parse_errors.is_empty() && file.syntax_violations.is_empty() => file,
        _ => {
            return Evaluation {
                status: Status::Malformed,
                r: RResult {
                    class: Status::Malformed,
                    exit: None,
                    stdout: String::new(),
                    stderr: String::new(),
                },
                diagnostics: Vec::new(),
            };
        }
    };
    let mut checker = Checker::new("case.R");
    checker.check(&file);
    let diagnostics: Vec<Value> = checker
        .take_diagnostics()
        .into_iter()
        .map(|diagnostic| {
            json!({
                "code": diagnostic.code,
                "start": diagnostic.span.start,
                "end": diagnostic.span.end,
                "severity": format!("{:?}", diagnostic.severity),
            })
        })
        .collect();
    let r = run_r(case, source, rscript, sandbox, WALL_LIMIT);
    if r.class != Status::Agreement {
        return Evaluation {
            status: match r.class {
                Status::RuntimeOtherError => Status::UnsupportedPremise,
                Status::RUnavailable => Status::RUnavailable,
                Status::TimedOut => Status::TimedOut,
                Status::Crashed => Status::Crashed,
                Status::OutputLimit => Status::OutputLimit,
                _ => Status::HarnessError,
            },
            r,
            diagnostics,
        };
    }
    let expected = case.expected_code();
    let condition_start = case.condition_start();
    let matching = diagnostics.iter().any(|diagnostic| {
        Some(diagnostic["code"].as_str().unwrap_or_default()) == expected
            && diagnostic["start"].as_u64() == Some(condition_start as u64)
    });
    let status = if let Some(code) = expected {
        if matching && !(inject_missing_ry002 && code == "RY002") {
            Status::Agreement
        } else {
            Status::Disagreement("missing expected rule at condition span")
        }
    } else if diagnostics
        .iter()
        .any(|d| matches!(d["code"].as_str(), Some("RY001" | "RY002")))
    {
        Status::Disagreement("cardinality rule on valid scalar control")
    } else {
        Status::Agreement
    };
    Evaluation {
        status,
        r,
        diagnostics,
    }
}

fn evaluate(case: &Case, rscript: &Path, sandbox: bool, inject_missing_ry002: bool) -> Evaluation {
    evaluate_source(case, &case.source(), rscript, sandbox, inject_missing_ry002)
}

fn shrink(
    original: &Case,
    rscript: &Path,
    sandbox: bool,
    inject_missing_ry002: bool,
) -> Option<Case> {
    let fingerprint = evaluate(original, rscript, sandbox, inject_missing_ry002).status;
    if !matches!(fingerprint, Status::Disagreement(_)) {
        return None;
    }
    let mut reduced = original.clone();
    for edit in 0..3 {
        let mut candidate = reduced.clone();
        match edit {
            0 => candidate.alias = false,
            1 => candidate.spaced = false,
            2 => candidate.alternate_literal = false,
            _ => unreachable!(),
        }
        candidate.seed = candidate.encoded_seed();
        // Retain precisely the same rule premise and disagreement. A parse
        // error, missing R, or unrelated R error can never pass this guard.
        if candidate.source().len() < reduced.source().len()
            && candidate.premise() == original.premise()
            && candidate.expected_code() == original.expected_code()
            && candidate.control == original.control
            && candidate.cardinality == original.cardinality
            && evaluate(&candidate, rscript, sandbox, inject_missing_ry002).status == fingerprint
        {
            reduced = candidate;
        }
    }
    Some(reduced)
}

fn rscript_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("Rscript"))
        .find(|path| path.is_file())
        .and_then(|path| fs::canonicalize(path).ok())
}

fn version(program: &Path) -> String {
    Command::new(program)
        .arg("--version")
        .output()
        .map(|output| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .to_owned()
        })
        .unwrap_or_else(|error| format!("unavailable: {error}"))
}

fn record(case: &Case, evaluation: &Evaluation, r_version: &str) -> Value {
    assert_eq!(
        Case::from_seed(case.seed),
        *case,
        "artifact seed must replay its source"
    );
    static RUSTC_VERSION: OnceLock<String> = OnceLock::new();
    let rustc_version = RUSTC_VERSION.get_or_init(|| version(Path::new("rustc")));
    json!({
        "generator_version": GENERATOR_VERSION,
        "seed": case.seed,
        "source": case.source(),
        "control": format!("{:?}", case.control),
        "cardinality": format!("{:?}", case.cardinality),
        "transformations": {
            "local_alias": case.alias,
            "extra_spacing": case.spaced,
            "alternate_literal": case.alternate_literal,
        },
        "premise": case.premise(),
        "expected": {
            "code": case.expected_code(),
            "condition_start_byte": case.condition_start(),
            "relationship": "diagnostic starts at generated condition",
        },
        "r_assertions": assertion_script(case),
        "r_result": {
            "class": format!("{:?}", evaluation.r.class),
            "exit": evaluation.r.exit,
            "stdout": evaluation.r.stdout,
            "stderr": evaluation.r.stderr,
        },
        "checker_result": evaluation.diagnostics,
        "status": format!("{:?}", evaluation.status),
        "tool_versions": {
            "rscript": r_version,
            "ry": env!("CARGO_PKG_VERSION"),
            "rustc": rustc_version,
        },
    })
}

fn artifact_dir() -> PathBuf {
    std::env::var_os("RY_SEMANTIC_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/semantic-generation")
        })
}

fn write_artifact(name: &str, value: &Value) {
    let dir = artifact_dir();
    fs::create_dir_all(&dir).expect("create semantic artifact directory");
    fs::write(
        dir.join(name),
        serde_json::to_vec_pretty(value).expect("serialize semantic artifact"),
    )
    .expect("write semantic artifact");
}

#[test]
fn generator_is_versioned_deterministic_and_covers_controls() {
    assert_eq!(GENERATOR_VERSION, "condition-cardinality-v1");
    let mut sources = std::collections::HashSet::new();
    for seed in 0..64 {
        assert_eq!(
            Case::from_seed(seed).source(),
            Case::from_seed(seed).source()
        );
        assert_eq!(Case::from_seed(seed).encoded_seed(), seed);
        assert!(sources.insert(Case::from_seed(seed).source()));
    }
    for seed in 0..8 {
        let case = Case::from_seed(seed);
        assert!(!case.alias);
        assert!(!case.spaced);
        assert!(!case.alternate_literal);
    }
    assert_eq!(Case::from_seed(62).expected_code(), Some("RY002"));
    assert_eq!(
        Case::from_seed(62).source(),
        "condition <- c(TRUE, FALSE, TRUE)\nif  (condition)  TRUE else FALSE\n"
    );
}

#[test]
fn malformed_and_unrelated_runtime_failure_are_not_disagreements() {
    let mut parser = RParser::new().expect("parser");
    let malformed = parser
        .parse("broken.R", "if (TRUE\n")
        .expect("parse result");
    assert!(!malformed.parse_errors.is_empty());
    // A missing package and arbitrary R error have no place in the audited
    // generator grammar. Even if a caller handed one to R, only the exact
    // rule-specific success marker can establish the premise.
    assert!(!Case::from_seed(6).source().contains("library("));
    assert!(!Case::from_seed(6).source().contains("stop("));
}

#[cfg(unix)]
#[test]
fn process_failures_have_distinct_classes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("temporary scripts");
    let case = Case::from_seed(6);
    let script = dir.path().join("fake-rscript");
    let fake = |body: &str| {
        fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("write fake R");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("chmod fake R");
        run_r(
            &case,
            &case.source(),
            &script,
            false,
            Duration::from_millis(100),
        )
    };
    assert_eq!(
        run_r(
            &case,
            &case.source(),
            &dir.path().join("missing"),
            false,
            Duration::from_millis(100)
        )
        .class,
        Status::RUnavailable
    );
    assert_eq!(fake("while :; do :; done").class, Status::TimedOut);
    assert_eq!(fake("kill -SEGV $$").class, Status::Crashed);
    assert_eq!(fake("head -c 20000 /dev/zero").class, Status::OutputLimit);
    assert_eq!(fake("exit 7").class, Status::HarnessError);
}

#[test]
fn small_r_semantic_batch() {
    let required = std::env::var_os("RY_SEMANTIC_R_REQUIRED").is_some();
    let Some(rscript) = rscript_path() else {
        assert!(
            !required,
            "required semantic generation: Rscript unavailable"
        );
        eprintln!(
            "semantic generation R batch skipped: Rscript unavailable (set RY_SEMANTIC_R_REQUIRED=1 in the oracle gate)"
        );
        return;
    };
    let r_version = version(&rscript);
    let reference = Case::from_seed(6);
    assert_eq!(
        evaluate_source(&reference, "if (TRUE\n", &rscript, false, false).status,
        Status::Malformed
    );
    for unrelated in [
        "library(ry_definitely_missing_generator_package_547)\nif (c(TRUE, FALSE)) TRUE\n",
        "stop('unrelated R error')\nif (c(TRUE, FALSE)) TRUE\n",
    ] {
        let result = evaluate_source(&reference, unrelated, &rscript, false, true);
        assert_eq!(
            result.status,
            Status::UnsupportedPremise,
            "unrelated R failure cannot satisfy the injected RY002 disagreement: {:?}",
            result.r
        );
    }
    let mut failures = Vec::new();
    for seed in 0..16 {
        let case = Case::from_seed(seed);
        let evaluation = evaluate(&case, &rscript, false, false);
        let artifact = record(&case, &evaluation, &r_version);
        write_artifact(&format!("seed-{seed:03}.json"), &artifact);
        if evaluation.status != Status::Agreement {
            failures.push(format!("seed {seed}: {:?}", evaluation.status));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));

    // Deliberately hide a real RY002 finding after checking it exists.
    // Reduction must keep the R multi-value premise, same code, and the
    // missing-diagnostic fingerprint. Both sources are retained.
    let original = Case::from_seed(62);
    let actual = evaluate(&original, &rscript, false, false);
    assert_eq!(actual.status, Status::Agreement);
    let reduced = shrink(&original, &rscript, false, true).expect("injected disagreement");
    assert!(reduced.source().len() < original.source().len());
    assert_eq!(reduced.premise(), original.premise());
    assert_eq!(reduced.expected_code(), Some("RY002"));
    let reduced_result = evaluate(&reduced, &rscript, false, true);
    assert_eq!(
        reduced_result.status,
        Status::Disagreement("missing expected rule at condition span")
    );
    write_artifact(
        "injected-disagreement.json",
        &json!({
            "kind": "injected missing RY002 after a real checker finding",
            "original": record(&original, &evaluate(&original, &rscript, false, true), &r_version),
            "reduced": record(&reduced, &reduced_result, &r_version),
        }),
    );
}

#[test]
#[ignore = "separately budgeted; requires working bwrap no-network sandbox"]
fn sandboxed_campaign() {
    let rscript = rscript_path().expect("campaign requires Rscript");
    let seeds: u64 = std::env::var("RY_SEMANTIC_CAMPAIGN_SEEDS")
        .expect("set RY_SEMANTIC_CAMPAIGN_SEEDS (1..=64)")
        .parse()
        .expect("campaign seed count must be integer");
    assert!((1..=64).contains(&seeds), "campaign count must be 1..=64");
    assert!(
        std::env::var_os("RY_SEMANTIC_ARTIFACT_DIR").is_some(),
        "campaign artifact directory required"
    );
    // A successful probe is mandatory; campaign execution never falls back
    // to unconfined R if bubblewrap is absent or blocked by the host.
    let probe = Command::new("bwrap")
        .args([
            "--unshare-net",
            "--die-with-parent",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--",
            "/bin/true",
        ])
        .output()
        .expect("campaign requires bwrap");
    assert!(
        probe.status.success(),
        "bwrap no-network probe failed: {}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let r_version = version(&rscript);
    let mut failures = Vec::new();
    for seed in 0..seeds {
        let case = Case::from_seed(seed);
        let evaluation = evaluate(&case, &rscript, true, false);
        write_artifact(
            &format!("campaign-{seed:03}.json"),
            &record(&case, &evaluation, &r_version),
        );
        if matches!(evaluation.status, Status::Disagreement(_)) {
            let reduced = shrink(&case, &rscript, true, false).expect("reproduce disagreement");
            let reduced_result = evaluate(&reduced, &rscript, true, false);
            write_artifact(
                &format!("campaign-disagreement-{seed:03}.json"),
                &json!({
                    "original": record(&case, &evaluation, &r_version),
                    "reduced": record(&reduced, &reduced_result, &r_version),
                }),
            );
        }
        if evaluation.status != Status::Agreement {
            failures.push(format!("seed {seed}: {:?}", evaluation.status));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}
