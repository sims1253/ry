use ry_testkit::{CliProcess, FixtureProject, normalize_path};
use serde_json::Value;
use std::path::Path;

#[test]
fn shared_fixture_reaches_real_cli_subprocess() {
    let fixture = FixtureProject::from_fixture("shared").unwrap();
    let output = CliProcess::new(env!("CARGO_BIN_EXE_ry"))
        .check(
            &fixture,
            Path::new("R/diagnostic.R"),
            ["--output-format", "json"],
        )
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "RY002"),
        "shared fixture should publish RY002: {diagnostics:?}"
    );
    assert!(diagnostics.iter().all(|diagnostic| normalize_path(
        diagnostic["path"].as_str().unwrap(),
        fixture.root()
    ) == "R/diagnostic.R"));
}

/// A reader that closes stdout early (`ry check | head`) ends the command
/// without a panic (#626).
#[cfg(unix)]
#[test]
fn closed_stdout_does_not_panic() {
    use std::process::{Command, Stdio};

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("bad.R"), "x <- 1 + 'a'\n").unwrap();
    for format in ["full", "concise", "json"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ry"))
            .args(["check", "--exit-zero", "--output-format", format])
            .arg(temp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("panicked"), "{format}: {stderr}");
        assert_ne!(output.status.code(), Some(101), "{format}: {stderr}");
    }
}
