use ry_testkit::{
    AsyncJsonRpcClient, CliProcess, FixtureProject, JsonRpcProcess, ObservedPosition,
    PositionEncoding, normalize_path, normalize_position,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PublishedFix {
    start_line: u32,
    start_byte_column: u32,
    end_line: u32,
    end_byte_column: u32,
    replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Published {
    path: String,
    code: String,
    severity: String,
    message: String,
    line: u32,
    byte_column: u32,
    fix: Option<PublishedFix>,
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
            let fix = value.get("fix").map(|fix| {
                let start = byte_offset_position(&source, fix["start"].as_u64().unwrap() as usize);
                let end = byte_offset_position(&source, fix["end"].as_u64().unwrap() as usize);
                PublishedFix {
                    start_line: start.0,
                    start_byte_column: start.1,
                    end_line: end.0,
                    end_byte_column: end.1,
                    replacement: fix["replacement"].as_str().unwrap().to_string(),
                }
            });
            Published {
                path: relative,
                code: value["code"].as_str().unwrap().to_string(),
                severity: value["severity"].as_str().unwrap().to_string(),
                message: value["message"].as_str().unwrap().to_string(),
                line: position.line,
                byte_column: position.character,
                fix,
            }
        })
        .collect();
    diagnostics.sort();
    diagnostics
}

fn byte_offset_position(source: &str, offset: usize) -> (u32, u32) {
    assert!(offset <= source.len() && source.is_char_boundary(offset));
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map_or(0, |position| position + 1);
    (line, (offset - line_start) as u32)
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
            let fix = value.pointer("/data/fix").map(|fix| {
                let start = normalize_position(
                    &source,
                    &ObservedPosition {
                        line: fix["range"]["start"]["line"].as_u64().unwrap() as u32,
                        character: fix["range"]["start"]["character"].as_u64().unwrap() as u32,
                        encoding: PositionEncoding::Utf16,
                    },
                )
                .unwrap();
                let end = normalize_position(
                    &source,
                    &ObservedPosition {
                        line: fix["range"]["end"]["line"].as_u64().unwrap() as u32,
                        character: fix["range"]["end"]["character"].as_u64().unwrap() as u32,
                        encoding: PositionEncoding::Utf16,
                    },
                )
                .unwrap();
                PublishedFix {
                    start_line: start.line,
                    start_byte_column: start.character,
                    end_line: end.line,
                    end_byte_column: end.character,
                    replacement: fix["replacement"].as_str().unwrap().to_string(),
                }
            });
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
                fix,
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

#[test]
fn structured_fixes_have_cli_and_lsp_publication_parity() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file(
            "fixes.R",
            r#"emoji <- "😀"; length(xx = 1L)
f <- function(x) {
  a <- x && c(TRUE, FALSE)
  b <- x == NA
  c <- abs(x > 0L)
  if (class(x) == "wi\"dget") 1L else 2L
}
length(c(1L, 2L) > 0L)
args <- list(font = "mono")
identical(args["font"], "mono")
list(ok = 1L, "wi\"dget" <- 2L)
"#,
        )
        .unwrap();
    let cli = cli_diagnostics(&fixture, &[]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let lsp = runtime.block_on(run_with_diagnostics(&fixture, "fixes.R", json!({})));
    assert_eq!(lsp, cli);

    // RY090's fix follows an astral character on the same line. Its byte
    // column (24) differs from its LSP UTF-16 column (22), so parity here
    // exercises the conversion rather than merely crossing a prior line.
    let astral_fix = cli
        .iter()
        .find(|diagnostic| diagnostic.code == "RY090")
        .and_then(|diagnostic| diagnostic.fix.as_ref())
        .expect("RY090 after the astral character should publish a fix");
    assert_eq!(astral_fix.start_line, 0);
    assert_eq!(astral_fix.start_byte_column, 24);

    let fixed_codes = cli
        .iter()
        .filter_map(|diagnostic| diagnostic.fix.as_ref().map(|_| diagnostic.code.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        fixed_codes,
        [
            "RY032", "RY034", "RY090", "RY093", "RY100", "RY101", "RY102", "RY103"
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn indexed_unopened_diagnostics_use_the_checked_source_for_utf16_ranges_and_fixes() {
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
        assert_eq!(
            unknown_argument["data"]["fix"],
            json!({
                "range": {
                    "start": {"line": 0, "character": 22},
                    "end": {"line": 0, "character": 24}
                },
                "replacement": "x"
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
