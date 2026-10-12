use ry_testkit::{DriverError, FixtureProject, JsonRpcProcess, file_uri};
use serde_json::{Value, json};
use std::process::Command;
use std::time::Duration;

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

/// `ry server` exits on `exit` while the client still holds stdin open; the
/// exit code reports whether `shutdown` came first.
#[test]
fn server_exits_on_exit_notification_with_stdin_open() -> Result<(), DriverError> {
    for (shutdown_first, expected) in [(true, 0), (false, 1)] {
        let fixture = FixtureProject::empty()?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
        command.arg("server").current_dir(fixture.root());
        let mut client = JsonRpcProcess::spawn(&mut command)?;
        let id = client.request(
            "initialize",
            json!({"processId": null, "rootUri": file_uri(fixture.root())?, "capabilities": {}}),
        )?;
        client.receive_until(|message| message["id"] == id, 32)?;
        client.notify("initialized", json!({}))?;
        if shutdown_first {
            let id = client.request("shutdown", Value::Null)?;
            client.receive_until(|message| message["id"] == id, 32)?;
        }
        client.notify("exit", Value::Null)?;
        let status = client.wait_for_exit(Duration::from_secs(5))?;
        assert_eq!(
            status.code(),
            Some(expected),
            "shutdown before exit: {shutdown_first}"
        );
    }
    Ok(())
}
