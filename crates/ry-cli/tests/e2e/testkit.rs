use ry_testkit::{
    CliProcess, DriverError, FixtureProject, JsonRpcProcess, file_uri, normalize_path,
};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

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

/// The shipped `ry server` speaks clean JSON-RPC on stdout and shuts down
/// cleanly.
#[test]
fn shared_fixture_reaches_real_lsp_subprocess_without_stdout_corruption() -> Result<(), DriverError>
{
    let fixture = FixtureProject::from_fixture("shared")?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
    command.arg("server").current_dir(fixture.root());
    let path = fixture.path("R/diagnostic.R");
    let diagnostics = JsonRpcProcess::spawn(&mut command)?.open_and_publish(
        fixture.root(),
        &file_uri(&path)?,
        &std::fs::read_to_string(&path)?,
    )?;
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "RY002"),
        "shared fixture should publish RY002: {diagnostics:?}"
    );
    Ok(())
}
