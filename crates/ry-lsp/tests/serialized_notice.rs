use ry_testkit::{AsyncJsonRpcClient, FixtureProject, file_uri};
use serde_json::json;

#[test]
fn indexing_reports_degraded_serialized_scope_to_the_client() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file("DESCRIPTION", "Package: example\nVersion: 1.0\n")
                .unwrap();
            fixture
                .write_file("ry.toml", "max-serialized-bytes = 1\n")
                .unwrap();
            fixture.write_file("R/main.R", "sysdata\n").unwrap();
            fixture
                .write_file("R/sysdata.rda", b"over the configured cap")
                .unwrap();
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
                                .is_some_and(|text| text.contains("max-serialized-bytes"))
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
