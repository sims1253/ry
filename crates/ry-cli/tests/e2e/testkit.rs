use ry_testkit::{
    CliProcess, DriverError, FixtureProject, JsonRpcProcess, file_uri, normalize_path,
};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

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

/// `ry server` exits on `exit` while the client still holds stdin open; the
/// exit code reports whether `shutdown` came first.
#[test]
fn server_exits_on_exit_notification_with_stdin_open() -> Result<(), DriverError> {
    let shutdown = json!({"jsonrpc": "2.0", "id": "shutdown", "method": "shutdown"});
    let exit = json!({"jsonrpc": "2.0", "method": "exit"});
    for (case, expected) in [
        ("shutdown, then exit", 0),
        ("pipelined", 0),
        ("exit only", 1),
    ] {
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
        match case {
            "shutdown, then exit" => {
                client.send(&shutdown)?;
                client.receive_until(|message| message["id"] == "shutdown", 32)?;
                client.send(&exit)?;
            }
            // Both messages in one write: the shutdown response must still
            // reach the client before the process exits.
            "pipelined" => {
                client.send_pipelined(&[shutdown.clone(), exit.clone()])?;
                let response = client.receive_until(|message| message["id"] == "shutdown", 32)?;
                assert_eq!(response["result"], Value::Null, "{response}");
            }
            _ => client.send(&exit)?,
        }
        let status = client.wait_for_exit(Duration::from_secs(5))?;
        assert_eq!(status.code(), Some(expected), "{case}");
    }
    Ok(())
}
