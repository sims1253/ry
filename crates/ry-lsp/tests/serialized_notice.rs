use ry_testkit::{AsyncJsonRpcClient, FixtureProject, file_uri};
use serde_json::json;

fn assert_serialized_notice(bytes: &[u8], config: &str, reason: &str) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file("DESCRIPTION", "Package: example\nVersion: 1.0\n")
                .unwrap();
            fixture.write_file("ry.toml", config).unwrap();
            fixture.write_file("R/main.R", "sysdata\n").unwrap();
            fixture.write_file("R/sysdata.rda", bytes).unwrap();
            let (client, server) = tokio::io::duplex(128 * 1024);
            let (reader, writer) = tokio::io::split(server);
            let server = tokio::spawn(ry_lsp::run_with(reader, writer));
            let (reader, writer) = tokio::io::split(client);
            let mut client = AsyncJsonRpcClient::new(reader, writer);
            let root = file_uri(fixture.root()).unwrap();
            let id = client
                .request(
                    "initialize",
                    json!({
                        "processId": null, "rootUri": root, "capabilities": {},
                        "workspaceFolders": [{"uri": root, "name": "example"}]
                    }),
                )
                .await
                .unwrap();
            client
                .receive_until(|message| message["id"] == id, 20)
                .await
                .unwrap();
            client.notify("initialized", json!({})).await.unwrap();
            let notice = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client.receive_until(
                    |message| {
                        message["method"] == "window/logMessage"
                            && message["params"]["message"]
                                .as_str()
                                .is_some_and(|text| text.contains(reason))
                    },
                    20,
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(notice["params"]["type"], 2);
            assert!(
                notice["params"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("R/sysdata.rda")
            );
            drop(client);
            server.abort();
        });
}

#[test]
fn indexing_reports_byte_limit_with_the_source_path() {
    assert_serialized_notice(
        b"over the configured cap",
        "max-serialized-bytes = 1\n",
        "decoded-byte limit exceeded",
    );
}

#[test]
fn indexing_reports_parser_limit_with_the_same_reason_as_cli() {
    assert_serialized_notice(
        include_bytes!("../../../testdata/serialized/nested-limit.rda"),
        "max-serialized-bytes = 4096\n",
        "serialized parser resource limit exceeded",
    );
}

#[test]
fn repaired_serialized_scope_clears_the_notice_and_a_later_failure_reports_again() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file("DESCRIPTION", "Package: example\nVersion: 1.0\n")
                .unwrap();
            fixture.write_file("R/main.R", "sysdata\n").unwrap();
            let data_path = fixture.path("R/sysdata.rda");
            let nested = include_bytes!("../../../testdata/serialized/nested-limit.rda");
            fixture.write_file("R/sysdata.rda", nested).unwrap();
            let (client, server) = tokio::io::duplex(128 * 1024);
            let (reader, writer) = tokio::io::split(server);
            let server = tokio::spawn(ry_lsp::run_with(reader, writer));
            let (reader, writer) = tokio::io::split(client);
            let mut client = AsyncJsonRpcClient::new(reader, writer);
            let root = file_uri(fixture.root()).unwrap();
            let main = file_uri(&fixture.path("R/main.R")).unwrap();
            let serialized = file_uri(&data_path).unwrap();
            let id = client
                .request(
                    "initialize",
                    json!({
                        "processId": null, "rootUri": root, "capabilities": {},
                        "workspaceFolders": [{"uri": root, "name": "example"}]
                    }),
                )
                .await
                .unwrap();
            client
                .receive_until(|message| message["id"] == id, 20)
                .await
                .unwrap();
            client.notify("initialized", json!({})).await.unwrap();
            let is_limit_notice = |message: &serde_json::Value| {
                message["method"] == "window/logMessage"
                    && message["params"]["message"].as_str().is_some_and(|text| {
                        text.contains("serialized parser resource limit exceeded")
                    })
            };
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client.receive_until(is_limit_notice, 20),
            )
            .await
            .unwrap()
            .unwrap();

            client
                .notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {
                        "uri": main, "languageId": "r", "version": 1, "text": "sysdata\n"
                    }}),
                )
                .await
                .unwrap();
            client
                .receive_until(
                    |message| {
                        message["method"] == "textDocument/publishDiagnostics"
                            && message["params"]["uri"] == main
                    },
                    30,
                )
                .await
                .unwrap();

            std::fs::write(
                &data_path,
                include_bytes!("../../../testdata/serialized/empty.rda"),
            )
            .unwrap();
            client
                .notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": serialized, "type": 2}]}),
                )
                .await
                .unwrap();
            let mut repeated_notices = 0;
            let repaired = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client.receive_until(
                    |message| {
                        if is_limit_notice(message) {
                            repeated_notices += 1;
                        }
                        message["method"] == "textDocument/publishDiagnostics"
                            && message["params"]["uri"] == main
                            && message["params"]["diagnostics"]
                                .as_array()
                                .is_some_and(|diags| {
                                    diags.iter().any(|diag| {
                                        diag["code"] == "RY010"
                                            && diag["message"]
                                                .as_str()
                                                .is_some_and(|text| text.contains("sysdata"))
                                    })
                                })
                    },
                    40,
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                repeated_notices, 0,
                "repair must clear the notice: {repaired}"
            );

            std::fs::write(&data_path, nested).unwrap();
            client
                .notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": serialized, "type": 2}]}),
                )
                .await
                .unwrap();
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client.receive_until(is_limit_notice, 40),
            )
            .await
            .unwrap()
            .unwrap();
            drop(client);
            server.abort();
        });
}
