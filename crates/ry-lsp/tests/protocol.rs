use ry_testkit::{AsyncJsonRpcClient, CliProcess, FixtureProject, JsonRpcProcess};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

mod harness;

use harness::{Published, file_uri, published_from_cli_value, published_from_lsp, ry_binary};

fn cli_diagnostics(fixture: &FixtureProject, extra: &[&str]) -> Vec<Published> {
    let output = CliProcess::new(ry_binary())
        .check(
            fixture,
            fixture.root(),
            ["--output-format", "json"]
                .into_iter()
                .chain(extra.iter().copied()),
        )
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    let mut diagnostics: Vec<_> = values
        .into_iter()
        .map(|value| published_from_cli_value(&value, fixture.root()))
        .collect();
    diagnostics.sort();
    diagnostics
}

async fn run_with_diagnostics(
    fixture: &FixtureProject,
    target: &str,
    initialization_settings: Value,
) -> Vec<Published> {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
    let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);
    let root_uri = file_uri(fixture.root());
    let initialize_id = client
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "initializationOptions": {
                    "settings": [initialization_settings],
                    "globalSettings": {}
                },
                "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
            }),
        )
        .await
        .unwrap();
    client
        .receive_until(|m| m.get("id") == Some(&json!(initialize_id)), 16)
        .await
        .unwrap();
    client.notify("initialized", json!({})).await.unwrap();

    let path = fixture.path(target);
    let uri = file_uri(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    client
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "r", "version": 1, "text": text
            }}),
        )
        .await
        .unwrap();
    let publish = client
        .receive_until(
            |m| {
                m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                    && m.pointer("/params/uri") == Some(&json!(uri))
            },
            64,
        )
        .await
        .unwrap();
    let diagnostics = published_from_lsp(&publish, &path, fixture.root());

    let shutdown_id = client.request("shutdown", Value::Null).await.unwrap();
    client
        .receive_until(|m| m.get("id") == Some(&json!(shutdown_id)), 16)
        .await
        .unwrap();
    client.notify("exit", Value::Null).await.unwrap();
    drop(client);
    tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    diagnostics
}

fn settings(fixture: &FixtureProject, relative: &str) -> Value {
    let path = fixture.path(relative);
    if path.is_file() {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    } else {
        json!({})
    }
}

#[test]
fn cli_and_run_with_publish_the_same_single_root_matrix() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let cases = [
        ("filtering/ignore", "diagnostic.R", &[][..]),
        ("filtering/select", "diagnostic.R", &[][..]),
        ("filtering/extend-select", "diagnostic.R", &[][..]),
        ("filtering/error", "diagnostic.R", &[][..]),
        ("filtering/warn", "diagnostic.R", &[][..]),
        ("filtering/exclude", "diagnostic.R", &[][..]),
        ("filtering/baseline", "diagnostic.R", &[][..]),
        (
            "filtering/min-confidence",
            "diagnostic.R",
            &["--min-confidence", "high"][..],
        ),
        ("unicode", "R/non_ascii.R", &[][..]),
        ("complete-package", "R/imports.R", &[][..]),
        ("complete-package", "R/native.R", &[][..]),
        ("excluded-influence", "kept.R", &[][..]),
    ];
    for (fixture_name, target, cli_args) in cases {
        let fixture = FixtureProject::from_fixture(fixture_name).unwrap();
        let settings = settings(&fixture, "lsp-settings.json");
        let cli = cli_diagnostics(&fixture, cli_args)
            .into_iter()
            .filter(|d| d.path == target)
            .collect::<Vec<_>>();
        let lsp = runtime.block_on(run_with_diagnostics(&fixture, target, settings));
        assert_eq!(lsp, cli, "published diagnostics differ for {fixture_name}");
    }
}

/// (#46): diagnostics for indexed files that were never opened use
/// the checked source for their UTF-16 ranges, not the (absent) in-memory
/// document text.
#[test]
fn indexed_unopened_diagnostics_use_the_checked_source_for_utf16_ranges() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("disk.R", "emoji <- \"😀\"; length(xx = 1L)\r\n")
        .unwrap();
    fixture.write_file("trigger.R", "ok <- 1L\n").unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server =
            tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
        let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);
        let root_uri = file_uri(fixture.root());
        let initialize_id = client
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
                }),
            )
            .await
            .unwrap();
        client
            .receive_until(
                |message| message.get("id") == Some(&json!(initialize_id)),
                16,
            )
            .await
            .unwrap();
        client.notify("initialized", json!({})).await.unwrap();

        let trigger_path = fixture.path("trigger.R");
        let trigger_uri = file_uri(&trigger_path);
        let trigger_text = std::fs::read_to_string(&trigger_path).unwrap();
        client
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": trigger_uri,
                    "languageId": "r",
                    "version": 1,
                    "text": trigger_text
                }}),
            )
            .await
            .unwrap();

        let disk_uri = file_uri(&fixture.path("disk.R"));
        let publication = client
            .receive_until(
                |message| {
                    message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                        && message.pointer("/params/uri") == Some(&json!(disk_uri))
                },
                64,
            )
            .await
            .unwrap();
        let diagnostics = publication["params"]["diagnostics"].as_array().unwrap();
        let diagnostic = |code: &str| {
            diagnostics
                .iter()
                .find(|diagnostic| diagnostic["code"] == code)
                .unwrap_or_else(|| panic!("missing {code} in {diagnostics:?}"))
        };

        let unknown_argument = diagnostic("RY090");
        assert_eq!(
            unknown_argument["range"],
            json!({
                "start": {"line": 0, "character": 22},
                "end": {"line": 0, "character": 29}
            })
        );

        let missing_argument = diagnostic("RY091");
        assert_eq!(
            missing_argument["range"],
            json!({
                "start": {"line": 0, "character": 15},
                "end": {"line": 0, "character": 30}
            })
        );

        let shutdown_id = client.request("shutdown", Value::Null).await.unwrap();
        client
            .receive_until(|message| message.get("id") == Some(&json!(shutdown_id)), 16)
            .await
            .unwrap();
        client.notify("exit", Value::Null).await.unwrap();
        drop(client);
        tokio::time::timeout(std::time::Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    });
}

#[test]
fn default_disabled_rules_are_absent_in_both_modes() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("diagnostic.R", "if (1L) print(1)\n")
        .unwrap();
    let cli = cli_diagnostics(&fixture, &[]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let lsp = runtime.block_on(run_with_diagnostics(&fixture, "diagnostic.R", json!({})));
    assert_eq!(lsp, cli);
    assert!(lsp.is_empty());
}

#[test]
fn explicit_empty_select_disables_default_rules_in_both_modes() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", "select = []\n").unwrap();
    fixture
        .write_file("diagnostic.R", "x <- missing_name\n")
        .unwrap();
    let cli = cli_diagnostics(&fixture, &[]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let lsp = runtime.block_on(run_with_diagnostics(&fixture, "diagnostic.R", json!({})));
    assert_eq!(lsp, cli);
    assert!(lsp.is_empty());
}

#[test]
fn package_import_from_value_position_is_clean_in_both_modes() {
    let fixture = FixtureProject::from_fixture("complete-package").unwrap();
    let cli = cli_diagnostics(&fixture, &[]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let lsp = runtime.block_on(run_with_diagnostics(&fixture, "R/imports.R", json!({})));
    assert!(
        !cli.iter()
            .any(|d| d.code == "RY010" && d.message.contains("imported_helper"))
    );
    assert!(
        !lsp.iter()
            .any(|d| d.code == "RY010" && d.message.contains("imported_helper"))
    );
}

/// cross-mode subprocess framing is correct over a multi-message
/// exchange with the real `ry server` process.
///
/// The full request/notification/response/exit cycle — initialize
/// included — must survive Content-Length framing without truncation,
/// merging, or leftover stdout noise. A regression that interleaves a
/// log line or uses a wrong Content-Length would fail here, not only in
/// an editor integration.
#[test]
fn cross_mode_subprocess_framing_survives_multi_round_exchange() {
    let fixture = FixtureProject::from_fixture("shared").unwrap();
    let mut command = Command::new(ry_binary());
    command.arg("server").current_dir(fixture.root());
    let mut client = JsonRpcProcess::spawn(&mut command).unwrap();
    let root_uri = file_uri(fixture.root());

    // Round 1: initialize request → response.
    let init_id = client
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "initializationOptions": {
                    "settings": [{}],
                    "globalSettings": {}
                },
            }),
        )
        .unwrap();
    let init_response = client
        .receive_until(|m| m.get("id") == Some(&json!(init_id)), 8)
        .unwrap();
    assert_eq!(
        init_response.pointer("/result/serverInfo/name"),
        Some(&json!("ry")),
    );

    // Round 2: initialized notification (no response expected), then
    // didOpen → publishDiagnostics notification.
    client.notify("initialized", json!({})).unwrap();
    let path = fixture.path("R/diagnostic.R");
    let uri = file_uri(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    client
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "r", "version": 1, "text": text
            }}),
        )
        .unwrap();
    let publish = client
        .receive_until(
            |m| {
                m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                    && m.pointer("/params/uri") == Some(&json!(uri))
            },
            64,
        )
        .unwrap();
    // The diagnostics array must be present and well-formed (framing
    // correctness implies the full array survived intact).
    assert!(
        publish.pointer("/params/diagnostics").is_some(),
        "publishDiagnostics notification missing diagnostics array",
    );

    // Round 3: shutdown request → response, then exit notification.
    let shutdown_id = client.request("shutdown", Value::Null).unwrap();
    let shutdown_response = client
        .receive_until(|m| m.get("id") == Some(&json!(shutdown_id)), 8)
        .unwrap();
    assert_eq!(shutdown_response["result"], Value::Null);
    client.notify("exit", Value::Null).unwrap();
}

// ---- ry_binary path resolution ----

/// The Cargo-JSON path resolver must pick the `ry` bin artifact, skip
/// null-executable dependency artifacts, and anchor a relative report
/// at the workspace root (cargo resolves a relative `CARGO_TARGET_DIR`
/// against the build's working directory, not the test runner's cwd).
#[test]
fn ry_executable_from_cargo_json_picks_the_bin_artifact() {
    let json = concat!(
        r#"{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"ry_core"},"executable":null}"#,
        "\n",
        r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"ry"},"executable":"/abs/target/debug/ry"}"#,
        "\n",
        r#"{"reason":"build-finished","success":true}"#,
        "\n",
    );
    assert_eq!(
        harness::ry_executable_from_cargo_json(json.as_bytes()),
        Some(PathBuf::from("/abs/target/debug/ry")),
        "must report the bin artifact's executable path"
    );
}

/// On Windows (or any setup where cargo reports a relative executable
/// path) the result is anchored at the workspace root so the tests
/// spawn the artifact that was built regardless of their own cwd.
#[test]
fn ry_executable_from_cargo_json_anchors_relative_paths() {
    let json = concat!(
        r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"ry"},"executable":"custom-target/debug/ry.exe"}"#,
        "\n",
    );
    assert_eq!(
        harness::ry_executable_from_cargo_json(json.as_bytes()),
        Some(harness::workspace_root().join("custom-target/debug/ry.exe")),
        "a relative report must be anchored at the workspace root"
    );
}
