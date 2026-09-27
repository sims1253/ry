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
