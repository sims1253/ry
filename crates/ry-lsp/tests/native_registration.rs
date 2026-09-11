use ry_testkit::{AsyncJsonRpcClient, FixtureProject, file_uri};
use serde_json::{Value, json};

fn unbound(message: &Value, name: &str) -> bool {
    message["params"]["diagnostics"]
        .as_array()
        .is_some_and(|diagnostics| {
            diagnostics.iter().any(|diagnostic| {
                diagnostic["code"] == "RY010"
                    && diagnostic["message"]
                        .as_str()
                        .is_some_and(|text| text.contains(name))
            })
        })
}

#[test]
fn native_registration_edits_refresh_open_document_bindings() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture.write_file("DESCRIPTION", "Package: example\nVersion: 1.0\n").unwrap();
            fixture.write_file("NAMESPACE", "useDynLib(example, .registration=TRUE)\n").unwrap();
            let source = "native_entry_xyz\nother_missing\n";
            fixture.write_file("R/main.R", source).unwrap();
            fixture.write_file("src/init.c", "").unwrap();
            let (client, server) = tokio::io::duplex(128 * 1024);
            let (reader, writer) = tokio::io::split(server);
            let server = tokio::spawn(ry_lsp::run_with(reader, writer));
            let (reader, writer) = tokio::io::split(client);
            let mut client = AsyncJsonRpcClient::new(reader, writer);
            let root = file_uri(fixture.root()).unwrap();
            let uri = file_uri(&fixture.path("R/main.R")).unwrap();
            let id = client.request("initialize", json!({
                "processId": null, "rootUri": root, "capabilities": {},
                "workspaceFolders": [{"uri": root, "name": "example"}]
            })).await.unwrap();
            client.receive_until(|message| message["id"] == id, 20).await.unwrap();
            client.notify("initialized", json!({})).await.unwrap();
            client.notify("textDocument/didOpen", json!({"textDocument": {
                "uri": uri, "languageId": "r", "version": 1, "text": source
            }})).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10),
                client.receive_until(|message| {
                    message["method"] == "textDocument/publishDiagnostics"
                        && unbound(message, "native_entry_xyz") && unbound(message, "other_missing")
                }, 50)).await.unwrap().unwrap();
            let registration = r#"
static const R_CallMethodDef calls[] = { {"native_entry_xyz", (DL_FUNC)&implementation, 0}, {NULL, NULL, 0} };
void R_init_example(DllInfo *dll) { R_registerRoutines(dll, NULL, calls, NULL, NULL); }
"#;
            for (contents, missing) in [(registration, false), ("", true)] {
                fixture.write_file("src/init.c", contents).unwrap();
                client.notify("workspace/didChangeWatchedFiles", json!({"changes": [{
                    "uri": file_uri(&fixture.path("src/init.c")).unwrap(), "type": 2
                }]})).await.unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(10),
                    client.receive_until(|message| {
                        message["method"] == "textDocument/publishDiagnostics"
                            && unbound(message, "native_entry_xyz") == missing
                            && unbound(message, "other_missing")
                    }, 50)).await.unwrap().unwrap();
            }
            drop(client);
            server.abort();
        });
}
