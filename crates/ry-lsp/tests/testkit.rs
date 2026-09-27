use ry_testkit::{AsyncJsonRpcClient, DriverError, FixtureProject, file_uri};
use serde_json::{Value, json};

#[tokio::test]
async fn shared_fixture_reaches_fast_run_with_protocol_path() -> Result<(), DriverError> {
    let fixture = FixtureProject::from_fixture("shared")?;
    let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
    let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

    let root_uri = file_uri(fixture.root())?;
    let initialize_id = client
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "workspaceFolders": [{"uri": root_uri, "name": "shared"}]
            }),
        )
        .await?;
    client
        .receive_until(
            |message| message.get("id") == Some(&json!(initialize_id)),
            8,
        )
        .await?;
    client.notify("initialized", json!({})).await?;

    let path = fixture.path("R/diagnostic.R");
    let uri = file_uri(&path)?;
    let text = std::fs::read_to_string(&path)?;
    client
        .notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "r",
                    "version": 1,
                    "text": text,
                }
            }),
        )
        .await?;
    let publish = client
        .receive_until(
            |message| {
                message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                    && message.pointer("/params/uri") == Some(&json!(uri))
            },
            32,
        )
        .await?;
    assert_eq!(publish["params"]["uri"], json!(uri));
    let diagnostics = publish["params"]["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "RY002"),
        "shared fixture should publish RY002: {diagnostics:?}"
    );

    let shutdown_id = client.request("shutdown", Value::Null).await?;
    client
        .receive_until(|message| message.get("id") == Some(&json!(shutdown_id)), 8)
        .await?;
    client.notify("exit", Value::Null).await?;
    drop(client);
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .map_err(|_| "run_with did not stop after exit")???;
    Ok(())
}
