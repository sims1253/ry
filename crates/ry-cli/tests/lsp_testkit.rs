use ry_testkit::{DriverError, FixtureProject, JsonRpcProcess, file_uri};
use serde_json::{Value, json};
use std::process::Command;

#[test]
fn shared_fixture_reaches_real_lsp_subprocess_without_stdout_corruption() -> Result<(), DriverError>
{
    let fixture = FixtureProject::from_fixture("shared")?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
    command.arg("server").current_dir(fixture.root());
    let mut client = JsonRpcProcess::spawn(&mut command)?;
    let root_uri = file_uri(fixture.root())?;
    let initialize_id = client.request(
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {},
            "workspaceFolders": [{"uri": root_uri, "name": "shared"}]
        }),
    )?;
    client.receive_until(
        |message| message.get("id") == Some(&json!(initialize_id)),
        8,
    )?;
    client.notify("initialized", json!({}))?;

    let path = fixture.path("R/diagnostic.R");
    let uri = file_uri(&path)?;
    let text = std::fs::read_to_string(&path)?;
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "r",
                "version": 1,
                "text": text,
            }
        }),
    )?;
    let publish = client.receive_until(
        |message| {
            message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && message.pointer("/params/uri") == Some(&json!(uri))
        },
        32,
    )?;
    assert_eq!(publish["params"]["uri"], json!(uri));
    let diagnostics = publish["params"]["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "RY002"),
        "shared fixture should publish RY002: {diagnostics:?}"
    );

    let shutdown_id = client.request("shutdown", Value::Null)?;
    client.receive_until(|message| message.get("id") == Some(&json!(shutdown_id)), 8)?;
    client.notify("exit", Value::Null)?;
    Ok(())
}
