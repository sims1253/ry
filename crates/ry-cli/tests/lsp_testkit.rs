use ry_testkit::{DriverError, FixtureProject, JsonRpcProcess, file_uri};
use std::process::Command;

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
