use ry_testkit::{
    AsyncJsonRpcClient, CliProcess, FixtureProject, JsonRpcProcess, ObservedPosition,
    PositionEncoding, normalize_path, normalize_position,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Published {
    path: String,
    code: String,
    severity: String,
    message: String,
    line: u32,
    byte_column: u32,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn ry_binary() -> PathBuf {
    static BINARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BINARY
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .current_dir(workspace_root())
                .args(["build", "--quiet", "-p", "ry-cli"])
                .status()
                .expect("build the production ry binary for the protocol gate");
            assert!(status.success());
            workspace_root().join("target/debug/ry")
        })
        .clone()
}

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
        .map(|value| {
            let path = value["path"].as_str().unwrap();
            let relative = normalize_path(Path::new(path), fixture.root());
            let relative = relative.strip_prefix("./").unwrap_or(&relative).to_string();
            let source = std::fs::read_to_string(fixture.path(&relative)).unwrap();
            let scalar = ObservedPosition {
                line: value["line"].as_u64().unwrap() as u32 - 1,
                character: value["column"].as_u64().unwrap() as u32 - 1,
                encoding: PositionEncoding::UnicodeScalar,
            };
            let position = normalize_position(&source, &scalar).unwrap();
            Published {
                path: relative,
                code: value["code"].as_str().unwrap().to_string(),
                severity: value["severity"].as_str().unwrap().to_string(),
                message: value["message"].as_str().unwrap().to_string(),
                line: position.line,
                byte_column: position.character,
            }
        })
        .collect();
    diagnostics.sort();
    diagnostics
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
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
    let mut diagnostics = published_from_lsp(&publish, &path, fixture.root());

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
    diagnostics.sort();
    diagnostics
}

fn published_from_lsp(message: &Value, path: &Path, root: &Path) -> Vec<Published> {
    let relative = normalize_path(path, root);
    let source = std::fs::read_to_string(path).unwrap();
    message
        .pointer("/params/diagnostics")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|value| {
            let start = &value["range"]["start"];
            let position = normalize_position(
                &source,
                &ObservedPosition {
                    line: start["line"].as_u64().unwrap() as u32,
                    character: start["character"].as_u64().unwrap() as u32,
                    encoding: PositionEncoding::Utf16,
                },
            )
            .unwrap();
            Published {
                path: relative.clone(),
                code: value["code"].as_str().unwrap().to_string(),
                severity: match value["severity"].as_u64() {
                    Some(1) => "error",
                    Some(2) => "warning",
                    Some(3) => "info",
                    Some(4) => "hint",
                    _ => "unknown",
                }
                .to_string(),
                message: value["message"].as_str().unwrap().to_string(),
                line: position.line,
                byte_column: position.character,
            }
        })
        .collect()
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
fn actual_ry_server_stdio_is_clean_json_rpc() {
    let fixture = FixtureProject::from_fixture("shared").unwrap();
    let mut command = Command::new(ry_binary());
    command.arg("server").current_dir(fixture.root());
    let mut client = JsonRpcProcess::spawn(&mut command).unwrap();
    let root_uri = file_uri(fixture.root());
    let id = client
        .request(
            "initialize",
            json!({
                "processId": null, "rootUri": root_uri, "capabilities": {}
            }),
        )
        .unwrap();
    let response = client
        .receive_until(|m| m.get("id") == Some(&json!(id)), 8)
        .unwrap();
    assert_eq!(
        response.pointer("/result/serverInfo/name"),
        Some(&json!("ry"))
    );
    client.notify("exit", Value::Null).unwrap();
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

/// P37-W6 (#46): diagnostics for indexed files that were never opened use
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

/// P35-W11: cross-mode subprocess framing is correct over a multi-message
/// exchange with the real `ry server` process.
///
/// The existing `actual_ry_server_stdio_is_clean_json_rpc` test proves the
/// initialize response is framed. This test extends that to a full
/// request/notification/response/exit cycle and verifies every message
/// survives Content-Length framing without truncation, merging, or
/// leftover stdout noise. A regression that interleaves a log line or
/// uses a wrong Content-Length would fail here, not only in an editor
/// integration.
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
