//! End-to-end tests for `ry.toml` project configuration.
//!
//! These tests invoke the `ry` binary against temporary project trees
//! to exercise the full pipeline: discovery, parsing, merging with CLI
//! flags, and applying the merged settings to diagnostics. They
//! complement the unit tests in `src/config.rs`, which cover the
//! individual pieces in isolation.

use std::fs;
use std::process::Command;

/// Helper: run `ry check <arg>` and return the raw output.
fn ry_check(arg: &std::path::Path) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ry");
    Command::new(bin)
        .arg("check")
        .arg(arg)
        .output()
        .expect("failed to invoke ry binary")
}

/// Helper: run `ry check <arg>` from a specific working directory.
fn ry_check_in(cwd: &std::path::Path, arg: &std::path::Path) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ry");
    Command::new(bin)
        .current_dir(cwd)
        .arg("check")
        .arg(arg)
        .output()
        .expect("failed to invoke ry binary")
}

#[test]
fn ry_toml_applies_severity_overrides() {
    // RY002 (`if` on length-2 logical) defaults to a warning, so a
    // check against this file alone exits 0. Promoting RY002 to an
    // error via `ry.toml` must flip the exit code to failure and
    // surface the diagnostic as an error.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"error = ["RY002"]
"#,
    )
    .unwrap();
    // `if (c(TRUE, FALSE))` triggers RY002 (length-2 logical).
    fs::write(
        tmp.path().join("bad.R"),
        r#"if (c(TRUE, FALSE)) print(1)
"#,
    )
    .unwrap();

    let output = ry_check(tmp.path());
    assert!(
        !output.status.success(),
        "expected non-zero exit code (RY002 promoted to error), got {:?}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("RY002"),
        "expected RY002 in output: {}",
        stderr
    );
    assert!(
        stderr.contains("error"),
        "expected RY002 to be reported as an error, got: {}",
        stderr
    );
}

#[test]
fn ry_toml_without_error_override_keeps_warning_non_fatal() {
    // Sanity counterpart to the above: with no severity override, the
    // same file produces only a warning and the exit code is 0.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        r#"if (c(TRUE, FALSE)) print(1)
"#,
    )
    .unwrap();
    let output = ry_check(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("RY002"),
        "expected RY002 warning even without config: {}",
        stderr
    );
    assert!(
        output.status.success(),
        "warnings must not fail the run without error-on-warning, got {:?}",
        output.status
    );
}

#[test]
fn ry_toml_exclude_patterns_skip_matched_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"exclude = ["fixtures/**"]
"#,
    )
    .unwrap();
    // A `fixtures/` file that WOULD trigger an error if checked.
    fs::create_dir_all(tmp.path().join("fixtures")).unwrap();
    fs::write(
        tmp.path().join("fixtures/bad.R"),
        // RY040 is a default-error rule; if the exclude fails, this
        // surfaces as an error and fails the run.
        r#"x <- "a" + 1L
"#,
    )
    .unwrap();
    // A clean file outside the exclude so the run has something to do.
    fs::write(
        tmp.path().join("good.R"),
        r#"x <- 1L + 2L
"#,
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "excluded file must not be checked, got {:?}; stderr={}",
        output.status,
        stderr
    );
    assert!(
        !stderr.contains("RY040"),
        "RY040 from excluded file must not appear: {}",
        stderr
    );
}

#[test]
fn ry_toml_output_format_json() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"output-format = "json"
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        r#"x <- "a" + 1L
"#,
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() || !output.status.success(),
        "exit code is not the point of this test"
    );
    // JSON output lands on stdout (per main.rs's routing) and must
    // parse as a JSON array containing the RY040 diagnostic.
    assert!(
        stdout.trim_start().starts_with('['),
        "expected JSON array on stdout, got: {}",
        stdout
    );
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(
        parsed
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "RY040"),
        "expected RY040 in JSON diagnostics: {}",
        parsed
    );
}

#[test]
fn ry_toml_discovered_from_subdirectory() {
    // ry.toml lives at the project root; `ry check` is invoked from a
    // nested subdirectory. Discovery must walk up and find it.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"error = ["RY002"]
"#,
    )
    .unwrap();
    let sub = tmp.path().join("src").join("deep");
    fs::create_dir_all(&sub).unwrap();
    fs::write(
        sub.join("bad.R"),
        r#"if (c(TRUE, FALSE)) print(1)
"#,
    )
    .unwrap();

    // Run `ry check .` from the deep subdirectory.
    let output = ry_check_in(&sub, std::path::Path::new("."));
    assert!(
        !output.status.success(),
        "expected config discovery from subdir to promote RY002 to error, got {:?}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn ry_toml_cli_flag_overrides_config_output_format() {
    // The config sets json; the CLI flag --output-format concise must
    // win (CLI overrides config for scalars).
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"output-format = "json"
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        r#"x <- "a" + 1L
"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path())
        .arg("--output-format")
        .arg("concise")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // concise format goes to stderr and is NOT a JSON array.
    assert!(
        stdout.is_empty(),
        "concise output must not appear on stdout (config json was overridden): {}",
        stdout
    );
    assert!(
        stderr.contains("RY040"),
        "expected RY040 on stderr in concise format: {}",
        stderr
    );
}

#[test]
fn ry_toml_cli_error_appends_to_config_errors() {
    // Config promotes RY002 to error; CLI additionally promotes RY010.
    // A file triggering BOTH must surface both as errors and fail.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"error = ["RY002"]
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        // RY002 from the if-condition; RY010 from the unbound ref.
        r#"if (c(TRUE, FALSE)) print(undefined_thing)
"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path())
        .arg("--error")
        .arg("RY010")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected failure with appended error rules, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RY002"), "missing RY002: {}", stderr);
    assert!(stderr.contains("RY010"), "missing RY010: {}", stderr);
}

#[test]
fn ry_toml_malformed_aborts_with_error() {
    // A syntactically broken ry.toml must surface an error and abort
    // rather than silently running with defaults.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("ry.toml"), "this is not = = valid toml\n").unwrap();
    fs::write(tmp.path().join("ok.R"), "x <- 1\n").unwrap();

    let output = ry_check(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "malformed config must abort with non-zero exit, got {:?}",
        output.status
    );
    assert!(
        stderr.contains("ry.toml") && stderr.to_lowercase().contains("pars"),
        "expected a parse error mentioning ry.toml, got: {}",
        stderr
    );
}
