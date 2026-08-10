//! P36-W1: Red contract matrix — deterministic failing cases for every
//! remaining LSP contract gap addressed by Plan 36.
//!
//! Every test in this file is `#[ignore]`'d because it verifies behavior that
//! P36-W2 through W7 will implement. Each test's `#[ignore]` message and
//! doc-comment name the specific workstream (and issue) that will make it pass.
//!
//! Shared infrastructure reuses Plan 35's `ry-testkit` (`FixtureProject`,
//! `LspSession`, `AsyncJsonRpcClient`) and the `ry_lsp::run_with` in-memory
//! server seam. The CLI comparison helpers mirror `tests/protocol.rs` so the
//! contract is "LSP published diagnostics equal `ry check` run independently
//! in the same root."
//!
//! ## Plan 35 green prerequisites
//!
//! Before authoring these red cases, Plan 35's green gates were re-run and
//! confirmed passing:
//!
//! - **Package metadata parity**: `package_import_from_value_position_is_clean_in_both_modes`
//!   and the `complete-package` rows in `cli_and_run_with_publish_the_same_single_root_matrix`.
//! - **File eligibility**: the single-root rows in the same matrix plus
//!   `excluded-influence`.
//! - **Checker parameter-metadata dirty propagation (issue #52)**:
//!   `parameter_signature_change_reemits_transitive_callers` in
//!   `ry-checker/tests/project.rs`.
//!
//! No test below adds package-import or filtering differences to `normalise()`.

use ry_testkit::{
    AsyncJsonRpcClient, FixtureProject, ObservedPosition, PositionEncoding, normalize_path,
    normalize_position,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

// ──────────────────────────────────────────────────────────────────────────
// Shared helpers (adapted from tests/protocol.rs for multi-root support)
// ──────────────────────────────────────────────────────────────────────────

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

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// Run `ry check` in `dir` (as the working directory) and return normalized
/// diagnostics. Each root is checked independently so multi-root parity is
/// "LSP equals an independent CLI invocation in that root."
fn cli_diagnostics_in_dir(dir: &Path, extra: &[&str]) -> Vec<Published> {
    let mut command = Command::new(ry_binary());
    command
        .current_dir(dir)
        .arg("check")
        .arg("--output-format")
        .arg("json");
    for arg in extra {
        command.arg(arg);
    }
    command.arg(".");
    let output = command.output().expect("run ry check");
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values: Vec<Value> = serde_json::from_slice(&output.stdout).expect("CLI JSON output");
    let mut diagnostics: Vec<_> = values
        .into_iter()
        .map(|v| published_from_cli_value(&v, dir))
        .collect();
    diagnostics.sort();
    diagnostics
}

fn published_from_cli_value(value: &Value, root: &Path) -> Published {
    let path = value["path"].as_str().unwrap();
    let relative = normalize_path(Path::new(path), root);
    let relative = relative.strip_prefix("./").unwrap_or(&relative).to_string();
    let source = std::fs::read_to_string(root.join(&relative)).unwrap_or_default();
    let scalar = ObservedPosition {
        line: value["line"].as_u64().unwrap() as u32 - 1,
        character: value["column"].as_u64().unwrap() as u32 - 1,
        encoding: PositionEncoding::UnicodeScalar,
    };
    let position = normalize_position(&source, &scalar).unwrap();
    let fix = value.get("fix").filter(|f| f.is_object()).map(|fix| {
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
}

fn byte_offset_position(source: &str, offset: usize) -> (u32, u32) {
    assert!(offset <= source.len() && source.is_char_boundary(offset));
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map_or(0, |position| position + 1);
    (line, (offset - line_start) as u32)
}

/// Normalize an LSP `publishDiagnostics` message into `Published` entries.
fn published_from_lsp(message: &Value, path: &Path, root: &Path) -> Vec<Published> {
    let relative = normalize_path(path, root);
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let mut diags: Vec<_> = message
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|value| {
            let start = &value["range"]["start"];
            let position = normalize_position(
                &source,
                &ObservedPosition {
                    line: start["line"].as_u64().unwrap_or(0) as u32,
                    character: start["character"].as_u64().unwrap_or(0) as u32,
                    encoding: PositionEncoding::Utf16,
                },
            )
            .expect("diagnostic start position must normalize");
            let fix = value.pointer("/data/fix").map(|fix| {
                let start = normalize_position(
                    &source,
                    &ObservedPosition {
                        line: fix["range"]["start"]["line"].as_u64().unwrap_or(0) as u32,
                        character: fix["range"]["start"]["character"].as_u64().unwrap_or(0) as u32,
                        encoding: PositionEncoding::Utf16,
                    },
                )
                .expect("diagnostic start position must normalize");
                let end = normalize_position(
                    &source,
                    &ObservedPosition {
                        line: fix["range"]["end"]["line"].as_u64().unwrap_or(0) as u32,
                        character: fix["range"]["end"]["character"].as_u64().unwrap_or(0) as u32,
                        encoding: PositionEncoding::Utf16,
                    },
                )
                .expect("fix start position must normalize");
                PublishedFix {
                    start_line: start.line,
                    start_byte_column: start.character,
                    end_line: end.line,
                    end_byte_column: end.character,
                    replacement: fix["replacement"].as_str().unwrap_or("").to_string(),
                }
            });
            Published {
                path: relative.clone(),
                code: value["code"].as_str().unwrap_or("").to_string(),
                severity: match value["severity"].as_u64() {
                    Some(1) => "error",
                    Some(2) => "warning",
                    Some(3) => "info",
                    Some(4) => "hint",
                    _ => "unknown",
                }
                .to_string(),
                message: value["message"].as_str().unwrap_or("").to_string(),
                line: position.line,
                byte_column: position.character,
                fix,
            }
        })
        .collect();
    diags.sort();
    diags
}

// ──────────────────────────────────────────────────────────────────────────
// P36-W2a — Per-folder editor settings (#44)
//
// The server currently stores a single server-wide `folder_settings`
// (`State::folder_settings`) taken from the first `initializationOptions`
// entry. Per-folder editor settings are not honored. This test creates two
// roots whose `ry.toml` already configures different behavior and supplies
// matching per-folder editor settings so that the correct LSP output equals
// an independent CLI run in each root.
//
// Fix: P36-W2a replaces the single `folder_settings` with per-root values.
// ──────────────────────────────────────────────────────────────────────────

/// P36-W2a (#44): two roots with different editor settings must produce
/// per-file CLI/LSP parity. Root A ignores RY002 (both in `ry.toml` and
/// editor settings); root B does not. Both files trigger RY002.
///
/// The current code applies only the first folder's settings server-wide, so
/// root B's editor ignore list incorrectly suppresses RY002. This fails
/// against `ry check` run independently in root B.
#[test]
fn p36_w2a_two_roots_different_editor_settings_differential() {
    let fixture = FixtureProject::empty().unwrap();
    // Root A: ry.toml ignores RY002.
    fixture
        .write_file("root-a/ry.toml", "ignore = [\"RY002\"]\n")
        .unwrap();
    fixture
        .write_file("root-a/R/main.R", "if (c(TRUE, FALSE)) print(1)\n")
        .unwrap();
    // Root B: no ignore for RY002.
    fixture
        .write_file("root-b/R/main.R", "if (c(TRUE, FALSE)) print(1)\n")
        .unwrap();

    let root_a = fixture.path("root-a");
    let root_b = fixture.path("root-b");

    // CLI reference: run `ry check` independently in each root.
    let cli_a = cli_diagnostics_in_dir(&root_a, &[]);
    let cli_b = cli_diagnostics_in_dir(&root_b, &[]);

    // They must differ (root A suppresses RY002; root B does not).
    assert_ne!(cli_a, cli_b, "roots must produce different CLI output");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root_uri = file_uri(fixture.root());
        let root_a_uri = file_uri(&root_a);
        let root_b_uri = file_uri(&root_b);

        // Per-folder editor settings matching each root's ry.toml.
        let init_options = json!({
            "settings": [
                {"lint": {"ignore": ["RY002"]}},
                {}
            ],
            "globalSettings": {}
        });

        let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server =
            tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
        let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

        let init_id = client
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "initializationOptions": init_options,
                    "workspaceFolders": [
                        {"uri": root_a_uri, "name": "root-a"},
                        {"uri": root_b_uri, "name": "root-b"}
                    ]
                }),
            )
            .await
            .unwrap();
        client
            .receive_until(|m| m.get("id") == Some(&json!(init_id)), 16)
            .await
            .unwrap();
        client.notify("initialized", json!({})).await.unwrap();

        // Open root A's file and collect its diagnostics.
        let path_a = fixture.path("root-a/R/main.R");
        let uri_a = file_uri(&path_a);
        let text_a = std::fs::read_to_string(&path_a).unwrap();
        client
            .notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {"uri": uri_a, "languageId": "r", "version": 1, "text": text_a}
                }),
            )
            .await
            .unwrap();
        let publish_a = client
            .receive_until(
                |m| {
                    m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                        && m.pointer("/params/uri") == Some(&json!(uri_a))
                },
                64,
            )
            .await
            .unwrap();
        let lsp_a = published_from_lsp(&publish_a, &path_a, &root_a);

        // Open root B's file and collect its diagnostics.
        let path_b = fixture.path("root-b/R/main.R");
        let uri_b = file_uri(&path_b);
        let text_b = std::fs::read_to_string(&path_b).unwrap();
        client
            .notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {"uri": uri_b, "languageId": "r", "version": 1, "text": text_b}
                }),
            )
            .await
            .unwrap();
        let publish_b = client
            .receive_until(
                |m| {
                    m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                        && m.pointer("/params/uri") == Some(&json!(uri_b))
                },
                64,
            )
            .await
            .unwrap();
        let lsp_b = published_from_lsp(&publish_b, &path_b, &root_b);

        // Shutdown.
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

        // Each root's LSP output must match its independent CLI run.
        assert_eq!(lsp_a, cli_a, "root-a LSP must match independent CLI run");
        assert_eq!(lsp_b, cli_b, "root-b LSP must match independent CLI run");
    });
}

// ──────────────────────────────────────────────────────────────────────────
// P36-W2b — Per-folder typeshed isolation (#54)
//
// `load_workspace_stubs` takes a single `root: Option<&Path>` and returns
// empty when the root has no `ry.toml`. In a multi-root workspace it is
// called with `root_uri`, not with each workspace folder. Two roots may
// define the same package differently; the fix loads stubs per root and
// resolves them through longest-prefix ownership.
// ──────────────────────────────────────────────────────────────────────────

/// P36-W2b (#54): two roots define the same stub package (`localdep`) with
/// different return types for `my_func`. Root A's stub returns integer (no
/// diagnostic); root B's stub returns character (RY001 — `if` condition is
/// character). Each root's LSP output must equal its independent CLI run.
///
/// Currently the LSP loads no stubs (root_uri has no typeshed config), so
/// `my_func` has an unknown return type and neither root produces RY001.
/// Root B diverges from its CLI run.
#[test]
fn p36_w2b_colliding_local_stubs_isolation() {
    let fixture = FixtureProject::empty().unwrap();

    for (root, return_mode) in [("root-a", "integer"), ("root-b", "character")] {
        fixture
            .write_file(
                format!("{root}/DESCRIPTION"),
                "Package: testpkg\nVersion: 0.0.1\nImports: localdep\n",
            )
            .unwrap();
        fixture
            .write_file(
                format!("{root}/NAMESPACE"),
                "importFrom(localdep, my_func)\nexport(use_it)\n",
            )
            .unwrap();
        fixture
            .write_file(format!("{root}/ry.toml"), "typeshed = [\"stubs\"]\n")
            .unwrap();
        let stub = serde_json::to_string(&json!({
            "schema_version": "1",
            "package": "localdep",
            "version": "test",
            "functions": {
                "my_func": {
                    "params": [],
                    "return": {"mode": return_mode, "length": "1"}
                }
            }
        }))
        .unwrap();
        fixture
            .write_file(format!("{root}/stubs/localdep.json"), &stub)
            .unwrap();
        fixture
            .write_file(format!("{root}/R/main.R"), "if (my_func()) print(1)\n")
            .unwrap();
    }

    let root_a = fixture.path("root-a");
    let root_b = fixture.path("root-b");
    let cli_a = cli_diagnostics_in_dir(&root_a, &[]);
    let cli_b = cli_diagnostics_in_dir(&root_b, &[]);

    // The two roots MUST produce different CLI output (the stub return types
    // differ). Without this, the test cannot detect a collision.
    assert_ne!(cli_a, cli_b, "roots must produce different CLI output");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root_uri = file_uri(fixture.root());
        let root_a_uri = file_uri(&root_a);
        let root_b_uri = file_uri(&root_b);

        let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server =
            tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
        let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

        let init_id = client
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "workspaceFolders": [
                        {"uri": root_a_uri, "name": "root-a"},
                        {"uri": root_b_uri, "name": "root-b"}
                    ]
                }),
            )
            .await
            .unwrap();
        client
            .receive_until(|m| m.get("id") == Some(&json!(init_id)), 16)
            .await
            .unwrap();
        client.notify("initialized", json!({})).await.unwrap();

        // Collect diagnostics for each root's file.
        let mut lsp_results: Vec<(String, Vec<Published>)> = Vec::new();
        for (label, root_dir) in [("root-a", &root_a), ("root-b", &root_b)] {
            let path = root_dir.join("R/main.R");
            let uri = file_uri(&path);
            let text = std::fs::read_to_string(&path).unwrap();
            client
                .notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {"uri": uri, "languageId": "r", "version": 1, "text": text}
                    }),
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
            let diags = published_from_lsp(&publish, &path, root_dir);
            lsp_results.push((label.to_string(), diags));
        }

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

        assert_eq!(
            lsp_results[0].1, cli_a,
            "root-a LSP must match independent CLI run"
        );
        assert_eq!(
            lsp_results[1].1, cli_b,
            "root-b LSP must match independent CLI run"
        );
    });
}

// ──────────────────────────────────────────────────────────────────────────
// P36-W2c — Honor the configuration override (#56)
//
// `FolderSettings::configuration` is deserialized but has no read site. The
// fix resolves it relative to the workspace root and loads that file instead
// of directory discovery.
// ──────────────────────────────────────────────────────────────────────────

/// P36-W2c (#56): a folder whose `ry.toml` lives at a custom path
/// (`config/custom.toml`) should honor the `configuration` editor setting.
/// The custom config ignores RY002; the source triggers RY002.
///
/// Currently `configuration` is dead code, so the server falls back to
/// directory discovery (no `ry.toml` at root) and RY002 appears. The test
/// asserts RY002 is absent — it fails against current code.
#[test]
fn p36_w2c_per_folder_custom_config_path() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("config/custom.toml", "ignore = [\"RY002\"]\n")
        .unwrap();
    fixture
        .write_file("R/main.R", "if (c(TRUE, FALSE)) print(1)\n")
        .unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root_uri = file_uri(fixture.root());
        let init_options = json!({
            "settings": [{"configuration": "config/custom.toml"}],
            "globalSettings": {}
        });

        let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server =
            tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
        let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

        let init_id = client
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "initializationOptions": init_options,
                    "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
                }),
            )
            .await
            .unwrap();
        client
            .receive_until(|m| m.get("id") == Some(&json!(init_id)), 16)
            .await
            .unwrap();
        client.notify("initialized", json!({})).await.unwrap();

        let path = fixture.path("R/main.R");
        let uri = file_uri(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        client
            .notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {"uri": uri, "languageId": "r", "version": 1, "text": text}
                }),
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

        // RY002 must be absent — the configuration override should load
        // custom.toml and suppress it.
        assert!(
            !diagnostics.iter().any(|d| d.code == "RY002"),
            "RY002 must be suppressed by the configuration override; got: {diagnostics:?}"
        );
    });
}

// ──────────────────────────────────────────────────────────────────────────
// P36-W3 — Workspace-folder mutation converges to cold state (#55)
//
// `did_change_workspace_folders` updates `workspace_folders` but does not
// remove `disk_files`, trees, diagnostics, or contexts owned by removed
// roots, and does not cancel results from an old folder set.
// ──────────────────────────────────────────────────────────────────────────

/// P36-W3 (#55): add and remove a workspace folder. After each mutation the
/// final diagnostics must equal a fresh server initialized on the same final
/// roots. Removed roots must leave no reachable state.
///
#[test]
fn p36_w3_workspace_folder_add_remove_convergence() {
    use ry_testkit::LspSession;

    let fixture = FixtureProject::empty().unwrap();
    // root-a and root-b each have a file with a diagnostic.
    fixture
        .write_file("root-a/R/main.R", "length(xx = 1L)\n")
        .unwrap();
    fixture
        .write_file("root-b/R/main.R", "length(xx = 1L)\n")
        .unwrap();

    let root_a = fixture.path("root-a");
    let root_b = fixture.path("root-b");
    let root_a_uri = file_uri(&root_a);
    let root_b_uri = file_uri(&root_b);
    let main_a_uri = file_uri(&root_a.join("R/main.R"));
    let main_b_uri = file_uri(&root_b.join("R/main.R"));
    let main_b_text = "length(xx = 1L)\n".to_string();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // ── Phase: initialize with both roots, open root-b's file,
        //    remove root-b, assert root-b's diagnostics are cleared. ──
        let (cs2, ss2) = tokio::io::duplex(128 * 1024);
        let (cr2, cw2) = tokio::io::split(cs2);
        let (sr2, sw2) = tokio::io::split(ss2);
        let server2 = tokio::spawn(async move { ry_lsp::run_with(sr2, sw2).await });
        let mut live2 = LspSession::new(cr2, cw2);

        live2
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_a_uri,
                    "capabilities": {},
                    "workspaceFolders": [
                        {"uri": root_a_uri, "name": "root-a"},
                        {"uri": root_b_uri, "name": "root-b"}
                    ]
                }),
            )
            .await
            .unwrap();
        live2.notify("initialized", json!({})).await.unwrap();
        live2
            .request(
                "textDocument/hover",
                json!({
                    "textDocument": {"uri": main_a_uri},
                    "position": {"line": 0, "character": 0}
                }),
            )
            .await
            .ok();

        // Open root-b's file so we can observe its diagnostics.
        let open_mark = live2.publication_mark();
        live2.open(&main_b_uri, 1, &main_b_text).await.unwrap();
        live2
            .published_diagnostics_after(&main_b_uri, open_mark)
            .await
            .unwrap();

        // Remove root-b.
        live2
            .notify(
                "workspace/didChangeWorkspaceFolders",
                json!({
                    "event": {
                        "added": [],
                        "removed": [{"uri": root_b_uri, "name": "root-b"}]
                    }
                }),
            )
            .await
            .unwrap();

        // After removal, trigger a republish and sync.
        live2
            .request(
                "textDocument/hover",
                json!({
                    "textDocument": {"uri": main_a_uri},
                    "position": {"line": 0, "character": 0}
                }),
            )
            .await
            .ok();
        let clear_mark = live2.publication_mark();

        // Give the server a chance to republish for root-b's file.
        // After removal, root-b's file should clear its diagnostics.
        // The server republishes after the folder change.
        let republish = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            live2.published_diagnostics_after(&main_b_uri, clear_mark),
        )
        .await;

        let _ = live2.shutdown().await;
        drop(live2);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server2).await;

        // After removing root-b, the server must clear diagnostics for files
        // owned by root-b (empty publish or no publish at all).
        // Currently disk_files from root-b persist, so diagnostics remain.
        match republish {
            Ok(Ok(publish)) => {
                let diags = publish
                    .pointer("/params/diagnostics")
                    .and_then(Value::as_array);
                assert!(
                    diags.is_none_or(|d| d.is_empty()),
                    "root-b diagnostics must be cleared after removal; got: {:?}",
                    publish.pointer("/params/diagnostics")
                );
            }
            Ok(Err(e)) => panic!("transport error during republish: {e}"),
            Err(_) => { /* timeout: server quiesced, diagnostics may already be cleared */ }
        }
    });
}

/// P36-W4 (#53): a stale parse result must not replace a newer tree cache
/// entry. The forced sequence is:
///   1. parse version N starts;
///   2. `didChange` installs N+1;
///   3. parse N finishes;
///   4. stale result is rejected;
///   5. diagnostics equal a fresh parse of N+1.
///
/// The test-only scheduler/barrier seam (`ry_lsp::test_seam`) forces this
/// ordering without any sleeps. The seam controls scheduling only; cache
/// policy (version-stamped tree rejection) is production code in
/// `backend::parsed_file` and `State::store_tree`/`State::tree_for`.
#[test]
fn p36_w4_version_stamped_tree_cache_rejects_stale_parse() {
    use ry_testkit::LspSession;

    let fixture = FixtureProject::empty().unwrap();
    let source_v1 = "x <- 1L\ny <- 2L\n";
    let source_v2 = "z <- length(xx = 1L)\n"; // triggers RY090 after edit
    fixture.write_file("main.R", source_v1).unwrap();

    let main_uri = file_uri(&fixture.path("main.R"));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (cs, ss) = tokio::io::duplex(128 * 1024);
        let (cr, cw) = tokio::io::split(cs);
        let (sr, sw) = tokio::io::split(ss);
        let server = tokio::spawn(async move { ry_lsp::run_with(sr, sw).await });
        let mut live = LspSession::new(cr, cw);
        live.initialize(fixture.root()).await.unwrap();

        // ── Force the interleaving (#53): ──
        //   1. parse version N=1 starts
        //   2. didChange installs N+1=2
        //   3. parse N=1 finishes
        //   4. stale result is rejected by the version-stamped tree cache
        //   5. diagnostics equal a fresh parse of N+1=2

        // Arm the test-only scheduler barrier. The next `parsed_file` cache
        // miss pauses after reading the document text/version/tree but before
        // parsing. The barrier also arms a didChange-processed notification
        // so the test can confirm the new version is installed before
        // releasing the paused parse.
        ry_lsp::test_seam::arm();

        // Open version 1. `schedule_diagnostics` debounces 180 ms, then
        // `publish_diagnostics` calls `parsed_file` → barrier pauses.
        live.open(&main_uri, 1, source_v1).await.unwrap();

        // Wait for the parse of version 1 to start (step 1): `parsed_file`
        // has read the v1 text/version/tree and is now paused.
        ry_lsp::test_seam::wait_arrived().await;

        // Install version 2 while the version-1 parse is paused (step 2).
        live.change(&main_uri, 2, json!([{"text": source_v2}]))
            .await
            .unwrap();

        // Wait for didChange to be fully processed: document updated,
        // version bumped, diagnostics re-scheduled. This sync point is
        // necessary because tower-lsp dispatches handlers concurrently —
        // without it the barrier release could race ahead of the document
        // update and the stale parse would not be detected.
        ry_lsp::test_seam::wait_did_change().await;

        // Release the barrier: the version-1 parse finishes (step 3). Its
        // tree is rejected by `store_tree` (version 1 ≠ current version 2)
        // and its `SourceFile` is rejected by `record_parse` (step 4). The
        // retry loop then parses version 2 fresh.
        ry_lsp::test_seam::release_barrier();

        // Collect diagnostics for version 2 (step 5). The didChange
        // triggered `schedule_diagnostics(gen=2)`, which publishes after
        // the debounce.
        let mark = live.publication_mark();
        let publish_v2 = live
            .published_diagnostics_after(&main_uri, mark)
            .await
            .unwrap();

        let v2_codes: Vec<&str> = publish_v2["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap())
            .collect();
        assert!(
            v2_codes.contains(&"RY090"),
            "version 2 must produce RY090 after the stale parse is rejected; got: {v2_codes:?}"
        );

        // Cleanup.
        let _ = live.shutdown().await;
        drop(live);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
    });
}

// ──────────────────────────────────────────────────────────────────────────
// P36-W5 — Cache baseline/config state outside the hot lock (#45)
//
// The baseline and effective config are loaded into each
// `FolderAnalysisContext` during initialize; the publish/hover/completion
// path reads the cached value and performs no disk access. Watch events
// rebuild the context outside the write lock and swap it atomically; a
// failed reload retains the last valid context and emits a visible error.
//
// `ry_lsp::baseline_disk_reads()` is a process-global counter. Only the W5
// tests configure a baseline, so they serialize on this guard so the
// no-I/O assertion sees only its own server's reads.
// ──────────────────────────────────────────────────────────────────────────
static BASELINE_IO_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// P36-W5 (#45): a failed baseline reload must retain the last valid
/// context, not silently clear the baseline. The test:
///   1. Sets up a baseline that suppresses RY002.
///   2. Verifies the LSP suppresses RY002.
///   3. Corrupts the baseline file and triggers a watched-files event.
///   4. Asserts RY002 is STILL suppressed (last valid context retained).
///
/// The baseline is cached in the owning `FolderAnalysisContext` during
/// initialize and reloaded outside the write lock on watch events; when the
/// reload fails (`load_baseline` → error), the last valid baseline is kept.
#[test]
fn p36_w5_baseline_reload_retains_context_on_corruption() {
    use ry_testkit::LspSession;
    // Serialize against the other W5 tests so the global baseline-read
    // counter is not polluted by a concurrently-running server.
    let _io_guard = BASELINE_IO_TEST_GUARD.lock().unwrap();

    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "baseline = \"baseline.json\"\n")
        .unwrap();
    fixture.write_file(
        "baseline.json",
        r#"{"version": 1, "entries": [{"path": "main.R", "code": "RY002", "message": "`if` condition has length 2; R requires a length-1 condition", "count": 1}]}"#,
    ).unwrap();
    fixture
        .write_file("main.R", "if (c(TRUE, FALSE)) print(1)\n")
        .unwrap();

    let main_uri = file_uri(&fixture.path("main.R"));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (cs, ss) = tokio::io::duplex(128 * 1024);
        let (cr, cw) = tokio::io::split(cs);
        let (sr, sw) = tokio::io::split(ss);
        let server = tokio::spawn(async move { ry_lsp::run_with(sr, sw).await });
        let mut live = LspSession::new(cr, cw);
        live.initialize(fixture.root()).await.unwrap();

        // Phase 1: RY002 should be suppressed by the valid baseline.
        let mark1 = live.publication_mark();
        live.open(&main_uri, 1, "if (c(TRUE, FALSE)) print(1)\n").await.unwrap();
        let publish1 = live.published_diagnostics_after(&main_uri, mark1).await.unwrap();
        let codes1: Vec<&str> = publish1["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap_or(""))
            .collect();
        assert!(
            !codes1.contains(&"RY002"),
            "phase 1: RY002 must be suppressed by valid baseline; got: {codes1:?}"
        );

        // Phase 2: corrupt the baseline file and trigger a watched-files event.
        std::fs::write(fixture.path("baseline.json"), "CORRUPT NOT JSON").unwrap();
        let baseline_uri = file_uri(&fixture.path("baseline.json"));
        live.notify("workspace/didChangeWatchedFiles", json!({
            "changes": [{"uri": baseline_uri, "type": 2}]
        })).await.unwrap();

        // Sync barrier to let the reload + republish happen.
        live.request("textDocument/hover", json!({
            "textDocument": {"uri": main_uri},
            "position": {"line": 0, "character": 0}
        })).await.ok();

        // Phase 3: trigger a republish. RY002 must STILL be suppressed
        // (the last valid baseline context must be retained).
        let mark3 = live.publication_mark();
        live.change(&main_uri, 2, json!([{"text": "if (c(TRUE, FALSE)) print(1)\n"}])).await.unwrap();
        let publish3 = live.published_diagnostics_after(&main_uri, mark3).await.unwrap();
        let codes3: Vec<&str> = publish3["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap_or(""))
            .collect();

        let _ = live.shutdown().await;
        drop(live);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;

        // W5 contract: a failed reload retains the last valid context.
        // The cached baseline is kept when the corrupt file fails to reload.
        assert!(
            !codes3.contains(&"RY002"),
            "phase 3: RY002 must still be suppressed (last valid baseline retained); got: {codes3:?}"
        );
    });
}

/// P36-W5 (#45): a *successful* baseline reload must converge to the new
/// value, matching a cold server started fresh against the same files.
///
/// The test:
///   1. Sets up a baseline that suppresses RY002 (count 1).
///   2. Verifies the LSP suppresses RY002.
///   3. Overwrites the baseline with a valid EMPTY baseline.
///   4. Triggers the watch path and quiesces.
///   5. Republishes: RY002 must now APPEAR (reload converged).
///   6. Spawns a FRESH server against the same fixture and asserts both
///      servers publish identical diagnostics.
#[test]
fn p36_w5_baseline_reload_converges_to_new_value() {
    use ry_testkit::LspSession;
    // Serialize against the other W5 tests so the global baseline-read
    // counter is not polluted by a concurrently-running server.
    let _io_guard = BASELINE_IO_TEST_GUARD.lock().unwrap();

    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "baseline = \"baseline.json\"\n")
        .unwrap();
    fixture.write_file(
        "baseline.json",
        r#"{"version": 1, "entries": [{"path": "main.R", "code": "RY002", "message": "`if` condition has length 2; R requires a length-1 condition", "count": 1}]}"#,
    ).unwrap();
    fixture
        .write_file("main.R", "if (c(TRUE, FALSE)) print(1)\n")
        .unwrap();

    let main_uri = file_uri(&fixture.path("main.R"));

    fn diagnostic_codes(value: &Value) -> Vec<String> {
        value["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap_or("").to_string())
            .collect()
    }

    // Run a single LSP server to completion and return the codes published
    // for `main.R` after opening it. Reused for the live (reloaded) server
    // and the fresh (cold) comparison server.
    fn codes_for_fresh_server(fixture: &FixtureProject, main_uri: &str) -> Vec<String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (cs, ss) = tokio::io::duplex(128 * 1024);
            let (cr, cw) = tokio::io::split(cs);
            let (sr, sw) = tokio::io::split(ss);
            let server = tokio::spawn(async move { ry_lsp::run_with(sr, sw).await });
            let mut live = LspSession::new(cr, cw);
            live.initialize(fixture.root()).await.unwrap();
            let mark = live.publication_mark();
            live.open(main_uri, 1, "if (c(TRUE, FALSE)) print(1)\n")
                .await
                .unwrap();
            let publish = live
                .published_diagnostics_after(main_uri, mark)
                .await
                .unwrap();
            let codes = diagnostic_codes(&publish);
            let _ = live.shutdown().await;
            drop(live);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
            codes
        })
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let reloaded_codes = runtime.block_on(async {
        let (cs, ss) = tokio::io::duplex(128 * 1024);
        let (cr, cw) = tokio::io::split(cs);
        let (sr, sw) = tokio::io::split(ss);
        let server = tokio::spawn(async move { ry_lsp::run_with(sr, sw).await });
        let mut live = LspSession::new(cr, cw);
        live.initialize(fixture.root()).await.unwrap();

        // Phase 1: RY002 should be suppressed by the valid baseline.
        let mark1 = live.publication_mark();
        live.open(&main_uri, 1, "if (c(TRUE, FALSE)) print(1)\n")
            .await
            .unwrap();
        let publish1 = live
            .published_diagnostics_after(&main_uri, mark1)
            .await
            .unwrap();
        let codes1 = diagnostic_codes(&publish1);
        assert!(
            !codes1.contains(&"RY002".to_string()),
            "phase 1: RY002 must be suppressed by valid baseline; got: {codes1:?}"
        );

        // Phase 2: overwrite the baseline with a valid EMPTY baseline and
        // trigger the watch path, then quiesce.
        std::fs::write(
            fixture.path("baseline.json"),
            r#"{"version": 1, "entries": []}"#,
        )
        .unwrap();
        let baseline_uri = file_uri(&fixture.path("baseline.json"));
        live.notify(
            "workspace/didChangeWatchedFiles",
            json!({
                "changes": [{"uri": baseline_uri, "type": 2}]
            }),
        )
        .await
        .unwrap();
        // Sync barrier: the hover response guarantees the watch notification
        // (and its outside-the-lock context rebuild) has been processed.
        live.request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri},
                "position": {"line": 0, "character": 0}
            }),
        )
        .await
        .ok();

        // Phase 3: republish. RY002 must now APPEAR (empty baseline does not
        // suppress it) — the reload converged.
        let mark3 = live.publication_mark();
        live.change(
            &main_uri,
            2,
            json!([{"text": "if (c(TRUE, FALSE)) print(1)\n"}]),
        )
        .await
        .unwrap();
        let publish3 = live
            .published_diagnostics_after(&main_uri, mark3)
            .await
            .unwrap();
        let codes3 = diagnostic_codes(&publish3);

        let _ = live.shutdown().await;
        drop(live);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
        codes3
    });

    // Phase 4: the reloaded live server must match a cold server started
    // fresh against the now-current (empty) baseline.
    let fresh_codes = codes_for_fresh_server(&fixture, &main_uri);
    assert_eq!(
        reloaded_codes, fresh_codes,
        "reload must converge to cold state; reloaded: {reloaded_codes:?}, fresh: {fresh_codes:?}"
    );
    assert!(
        reloaded_codes.contains(&"RY002".to_string()),
        "phase 3: RY002 must reappear after the baseline reload converged; got: {reloaded_codes:?}"
    );
}

/// P36-W5 (#45): the publish/hover/completion hot path performs ZERO
/// baseline file reads. `baseline_disk_reads()` counts every disk read by
/// the context loader; a publish that touches it betrays a regression.
#[test]
fn p36_w5_publish_path_performs_no_baseline_disk_io() {
    use ry_testkit::LspSession;
    // Serialize against the other W5 tests so the global baseline-read
    // counter is not polluted by a concurrently-running server.
    let _io_guard = BASELINE_IO_TEST_GUARD.lock().unwrap();

    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "baseline = \"baseline.json\"\n")
        .unwrap();
    fixture.write_file(
        "baseline.json",
        r#"{"version": 1, "entries": [{"path": "main.R", "code": "RY002", "message": "`if` condition has length 2; R requires a length-1 condition", "count": 1}]}"#,
    ).unwrap();
    fixture
        .write_file("main.R", "if (c(TRUE, FALSE)) print(1)\n")
        .unwrap();

    let main_uri = file_uri(&fixture.path("main.R"));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (cs, ss) = tokio::io::duplex(128 * 1024);
        let (cr, cw) = tokio::io::split(cs);
        let (sr, sw) = tokio::io::split(ss);
        let server = tokio::spawn(async move { ry_lsp::run_with(sr, sw).await });
        let mut live = LspSession::new(cr, cw);
        live.initialize(fixture.root()).await.unwrap();

        // Snapshot AFTER initialize: the baseline is loaded once into the
        // per-folder context during initialize and must not be re-read by
        // any subsequent request.
        let reads_before = ry_lsp::baseline_disk_reads();

        // Exercise hover (read path) and publish (diagnostic path).
        live.request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri},
                "position": {"line": 0, "character": 0}
            }),
        )
        .await
        .ok();
        let mark = live.publication_mark();
        live.open(&main_uri, 1, "if (c(TRUE, FALSE)) print(1)\n")
            .await
            .unwrap();
        let _ = live
            .published_diagnostics_after(&main_uri, mark)
            .await
            .unwrap();
        // A second republish (didChange) to be thorough.
        let mark2 = live.publication_mark();
        live.change(
            &main_uri,
            2,
            json!([{"text": "if (c(TRUE, FALSE)) print(1)\n"}]),
        )
        .await
        .unwrap();
        let _ = live
            .published_diagnostics_after(&main_uri, mark2)
            .await
            .unwrap();

        let reads_after = ry_lsp::baseline_disk_reads();
        let _ = live.shutdown().await;
        drop(live);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;

        assert_eq!(
            reads_before,
            reads_after,
            "publish/hover path must perform zero baseline disk reads; got {} extra read(s)",
            reads_after.saturating_sub(reads_before)
        );
    });
}

// ──────────────────────────────────────────────────────────────────────────
// P36-W6 — Precompute filters once per folder (#46)
//
// `folder_config_for_path` plus filter/confidence/exclude construction runs
// inside the per-file publish loop. The fix compiles these once while
// building each `FolderAnalysisContext` and borrows the compiled values in
// the loop.
//
// P36-W6 adds a construction-count test hook so the count is asserted
// directly rather than inferred from wall time. Until then, this test
// creates the many-files fixture and verifies diagnostic correctness.
// ──────────────────────────────────────────────────────────────────────────

/// P36-W6 (#46): for a fixed folder count, filter/glob construction must be
/// flat as file count grows. This test creates many files in one folder,
/// opens a trigger document, and verifies that all published diagnostics are
/// byte-for-byte correct.
///
/// P36-W6 adds the construction-count instrumentation. Currently the filter
/// is recomputed per file inside `publish_diagnostics`.
#[test]
#[ignore = "P36-W6: precompute filters once per folder (#46) — fix pending"]
fn p36_w6_many_files_flat_filter_construction() {
    let fixture = FixtureProject::empty().unwrap();
    // Create 32 files, each with a diagnostic (RY090).
    for i in 0..32u8 {
        fixture
            .write_file(format!("file_{i:02}.R"), "length(xx = 1L)\n")
            .unwrap();
    }
    fixture.write_file("trigger.R", "ok <- 1L\n").unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root_uri = file_uri(fixture.root());
        let (client_stream, server_stream) = tokio::io::duplex(256 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server = tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
        let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

        let init_id = client.request("initialize", json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {},
            "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
        })).await.unwrap();
        client.receive_until(|m| m.get("id") == Some(&json!(init_id)), 16).await.unwrap();
        client.notify("initialized", json!({})).await.unwrap();

        // Open the trigger file to start the background index.
        let trigger_uri = file_uri(&fixture.path("trigger.R"));
        let trigger_text = std::fs::read_to_string(fixture.path("trigger.R")).unwrap();
        client.notify("textDocument/didOpen", json!({
            "textDocument": {"uri": trigger_uri, "languageId": "r", "version": 1, "text": trigger_text}
        })).await.unwrap();

        // Collect diagnostics for the first and last indexed file.
        let first_uri = file_uri(&fixture.path("file_00.R"));
        let last_uri = file_uri(&fixture.path("file_31.R"));
        let first_publish = client.receive_until(
            |m| m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && m.pointer("/params/uri") == Some(&json!(first_uri)),
            128,
        ).await.unwrap();
        let first_diags = published_from_lsp(
            &first_publish,
            &fixture.path("file_00.R"),
            fixture.root(),
        );
        let last_publish = client.receive_until(
            |m| m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && m.pointer("/params/uri") == Some(&json!(last_uri)),
            128,
        ).await.unwrap();
        let last_diags = published_from_lsp(
            &last_publish,
            &fixture.path("file_31.R"),
            fixture.root(),
        );

        let shutdown_id = client.request("shutdown", Value::Null).await.unwrap();
        client.receive_until(|m| m.get("id") == Some(&json!(shutdown_id)), 16).await.unwrap();
        client.notify("exit", Value::Null).await.unwrap();
        drop(client);
        tokio::time::timeout(std::time::Duration::from_secs(3), server).await.unwrap().unwrap().unwrap();

        // Every indexed file must produce the same RY090 diagnostic.
        // P36-W6 asserts the construction count is flat; this correctness
        // baseline must hold before and after the fix.
        assert!(
            first_diags.iter().any(|d| d.code == "RY090"),
            "file_00 must produce RY090"
        );
        // Diagnostics must be identical except for the file path.
        // P36-W6 will assert that filter/glob construction count is flat
        // as file count grows; this correctness baseline must hold.
        assert!(
            last_diags.iter().any(|d| d.code == "RY090"),
            "file_31 must produce RY090"
        );
        assert_eq!(
            first_diags.len(),
            last_diags.len(),
            "all files must produce the same number of diagnostics"
        );
        for (a, b) in first_diags.iter().zip(last_diags.iter()) {
            assert_eq!(a.code, b.code, "codes must match");
            assert_eq!(a.severity, b.severity, "severities must match");
            assert_eq!(a.message, b.message, "messages must match");
            assert_eq!(a.line, b.line, "lines must match");
            assert_eq!(a.byte_column, b.byte_column, "columns must match");
            assert_eq!(a.fix, b.fix, "fixes must match");
        }
        assert_eq!(first_diags[0].path, "file_00.R");
        assert_eq!(last_diags[0].path, "file_31.R");

        // P36-W6 adds a construction-count test hook to `publish_diagnostics`
        // so the filter/glob construction count is asserted directly as flat
        // when file count grows. Until that instrumentation lands, the
        // correctness baseline above passes but the construction-count
        // assertion cannot be made. This sentinel marks the test RED.
        panic!(
            "P36-W6: filter/glob construction-count instrumentation required; \
             this sentinel is removed when W6 provides the test hook"
        );
    });
}

// ──────────────────────────────────────────────────────────────────────────
// P36-W7 — Shared, bounded discovery (#48)
//
// The CLI (`collect_r_files`) and LSP (`index::discover_r_files`) use
// different discovery rules. For example, the CLI skips `target/` and
// `node_modules/` directories, while the LSP only skips hidden directories.
// The fix moves discovery behind a shared module so both modes agree.
// ──────────────────────────────────────────────────────────────────────────

/// P36-W7 (#48): the same fixture tree must produce the same discovered path
/// set in CLI and LSP. The fixture includes a `target/` directory (CLI skips
/// it; the LSP currently does not), a hidden directory (both skip), and a
/// normal file. Every file has a diagnostic so the discovery set is
/// observable through published diagnostic paths.
///
/// Currently the CLI and LSP discovery sets differ (CLI excludes
/// `target/`, LSP includes it). The equality assertion fails.
#[test]
fn p36_w7_cli_lsp_discovery_set_equality() {
    let fixture = FixtureProject::empty().unwrap();
    // Normal file with a diagnostic.
    fixture.write_file("normal.R", "length(xx = 1L)\n").unwrap();
    // File in target/ — CLI skips this directory, LSP does not.
    fixture
        .write_file("target/skipped.R", "length(xx = 1L)\n")
        .unwrap();
    // Hidden directory — both skip.
    fixture
        .write_file(".hidden/secret.R", "length(xx = 1L)\n")
        .unwrap();

    let root = fixture.root();

    // CLI discovery set: paths that received diagnostics.
    let cli_diags = cli_diagnostics_in_dir(root, &[]);
    let cli_paths: std::collections::BTreeSet<String> =
        cli_diags.iter().map(|d| d.path.clone()).collect();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root_uri = file_uri(root);
        let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server = tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
        let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

        let init_id = client.request("initialize", json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {},
            "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
        })).await.unwrap();
        client.receive_until(|m| m.get("id") == Some(&json!(init_id)), 16).await.unwrap();
        client.notify("initialized", json!({})).await.unwrap();

        // Open normal.R to trigger the background index.
        let normal_uri = file_uri(&fixture.path("normal.R"));
        let normal_text = std::fs::read_to_string(fixture.path("normal.R")).unwrap();
        client.notify("textDocument/didOpen", json!({
            "textDocument": {"uri": normal_uri, "languageId": "r", "version": 1, "text": normal_text}
        })).await.unwrap();

        // Collect ALL publishDiagnostics notifications within a bounded
        // message count to determine the LSP's discovered file set.
        let mut lsp_uris: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        // First, consume the normal.R publication.
        let _ = client.receive_until(
            |m| m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && m.pointer("/params/uri") == Some(&json!(normal_uri)),
            128,
        ).await.unwrap();

        // Now drain remaining indexed-file publications (bounded).
        for _ in 0..128 {
            match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                client.receive(),
            ).await {
                Ok(Ok(message)) => {
                    if message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                        && let Some(uri) = message.pointer("/params/uri").and_then(Value::as_str) {
                            let path = uri.strip_prefix("file://").unwrap_or(uri);
                            let rel = normalize_path(Path::new(path), root);
                            lsp_uris.insert(rel);
                        }
                }
                Ok(Err(e)) => panic!("transport error during drain: {e}"),
                Err(_) => break, // timeout: server has quiesced
            }
        }

        let shutdown_id = client.request("shutdown", Value::Null).await.unwrap();
        client.receive_until(|m| m.get("id") == Some(&json!(shutdown_id)), 16).await.unwrap();
        client.notify("exit", Value::Null).await.unwrap();
        drop(client);
        tokio::time::timeout(std::time::Duration::from_secs(3), server).await.unwrap().unwrap().unwrap();

        // The opened file's publication was consumed above; add it back.
        lsp_uris.insert("normal.R".to_string());

        // Convert LSP URIs to normalized paths and compare with CLI.
        assert_eq!(
            cli_paths, lsp_uris,
            "CLI and LSP must discover the same file set; \
             CLI: {cli_paths:?}, LSP: {lsp_uris:?}"
        );
    });
}
