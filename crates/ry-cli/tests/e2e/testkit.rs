use ry_testkit::{DriverError, FixtureProject, JsonRpcProcess, file_uri};
use serde_json::{Value, json};
use std::process::Command;
use std::time::Duration;

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
