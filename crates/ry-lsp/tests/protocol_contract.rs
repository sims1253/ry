//! Red contract matrix — deterministic failing cases for every
//! remaining LSP contract gap.
//!
//! Every case in this file was authored `#[ignore]`'d while its behavior was
//! unimplemented; those attributes were removed as the features
//! landed, and the tests now run as ordinary (passing) contract gates. Each
//! test's doc-comment names the behavior (and issue) it covers.
//!
//! Shared infrastructure reuses `ry-testkit` (`FixtureProject`,
//! `LspSession`, `AsyncJsonRpcClient`) and the `ry_lsp::run_with` in-memory
//! server seam. The CLI comparison helpers mirror `tests/protocol.rs` so the
//! contract is "LSP published diagnostics equal `ry check` run independently
//! in the same root."
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
    Published {
        path: relative,
        code: value["code"].as_str().unwrap().to_string(),
        severity: value["severity"].as_str().unwrap().to_string(),
        message: value["message"].as_str().unwrap().to_string(),
        line: position.line,
        byte_column: position.character,
    }
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
            }
        })
        .collect();
    diags.sort();
    diags
}

// ──────────────────────────────────────────────────────────────────────────
// Per-folder editor settings (#44)
//
// Bug being pinned: the server stored a single server-wide `folder_settings`
// (`State::folder_settings`) taken from the first `initializationOptions`
// entry, so per-folder editor settings were not honored. W2a replaced it
// with per-root values.
// ──────────────────────────────────────────────────────────────────────────

/// (#44): two roots with different editor settings must produce
/// per-file CLI/LSP parity. Root A ignores RY002 (both in `ry.toml` and
/// editor settings); root B does not. Both files trigger RY002.
///
/// Before W2a, root A's ignore list was applied server-wide and wrongly
/// suppressed RY002 in root B, diverging from `ry check` run there.
#[test]
fn two_roots_different_editor_settings_differential() {
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
// Per-folder typeshed isolation (#54)
//
// Bug being pinned: stubs were loaded only for the root URI, so in a
// multi-root workspace two roots defining the same package differently
// collided. The fix loads stubs per root and resolves them through
// longest-prefix ownership.
// ──────────────────────────────────────────────────────────────────────────

/// (#54): two roots define the same stub package (`localdep`) with
/// different return types for `my_func`. Root A's stub returns integer (no
/// diagnostic); root B's stub returns character (RY001 — `if` condition is
/// character). Each root's LSP output must equal its independent CLI run.
///
/// Before W2b, neither root loaded its local stubs: `my_func` had an
/// unknown return type and neither root produced RY001.
#[test]
fn colliding_local_stubs_isolation() {
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
// Honor the configuration override (#56)
//
// Bug being pinned: `FolderSettings::configuration` was deserialized but
// never read. The fix resolves it relative to the workspace root and loads
// that file instead of directory discovery.
// ──────────────────────────────────────────────────────────────────────────

/// (#56): a folder whose `ry.toml` lives at a custom path
/// (`config/custom.toml`) should honor the `configuration` editor setting.
/// The custom config ignores RY002; the source triggers RY002. Asserts
/// RY002 is absent, proving the override was honored.
#[test]
fn per_folder_custom_config_path() {
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
// Workspace-folder mutation converges to cold state (#55)
//
// Bug being pinned: `did_change_workspace_folders` updated
// `workspace_folders` but left behind `disk_files`, trees, diagnostics,
// and contexts owned by removed roots.
// ──────────────────────────────────────────────────────────────────────────

/// (#55): add and remove a workspace folder. After each mutation the
/// final diagnostics must equal a fresh server initialized on the same final
/// roots. Removed roots must leave no reachable state.
#[test]
fn workspace_folder_add_remove_convergence() {
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
                "textDocument/inlayHint",
                json!({
                    "textDocument": {"uri": main_a_uri},
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
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
                "textDocument/inlayHint",
                json!({
                    "textDocument": {"uri": main_a_uri},
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
                }),
            )
            .await
            .ok();
        let clear_mark = live2.publication_mark();

        // Wait for a possible post-removal publication for root-b's file.
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

/// (#53): a stale parse result must not replace a newer tree cache
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
fn version_stamped_tree_cache_rejects_stale_parse() {
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

        // Force the parse/didChange interleaving (#53); the sequence is
        // documented at the `maybe_pause` call site in `backend::parsed_file`.
        ry_lsp::test_seam::arm();

        // Open version 1. `schedule_diagnostics` debounces 180 ms, then
        // `publish_diagnostics` calls `parsed_file` → barrier pauses.
        live.open(&main_uri, 1, source_v1).await.unwrap();

        // Wait for the parse of version 1 to start: `parsed_file` has read
        // the v1 text/version/tree and is now paused.
        ry_lsp::test_seam::wait_arrived().await;

        // Install version 2 while the version-1 parse is paused.
        live.change(&main_uri, 2, json!([{"text": source_v2}]))
            .await
            .unwrap();

        // Wait for didChange to be fully processed: document updated,
        // version bumped, diagnostics re-scheduled. This sync point is
        // necessary because tower-lsp dispatches handlers concurrently —
        // without it the barrier release could race ahead of the document
        // update and the stale parse would not be detected.
        ry_lsp::test_seam::wait_did_change().await;

        // Release the barrier: the version-1 parse finishes, its tree is
        // rejected by `store_tree` (version 1 ≠ current version 2) and its
        // `SourceFile` by `record_parse`, and the retry loop parses
        // version 2 fresh.
        ry_lsp::test_seam::release_barrier();

        // Collect diagnostics for version 2. The didChange triggered
        // `schedule_diagnostics(gen=2)`, which publishes after the debounce.
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
// Cache baseline/config state outside the hot lock (#45)
//
// The baseline and effective config are loaded into each
// `FolderAnalysisContext` during initialize; the publish/inlay-hint
// path reads the cached value and performs no disk access.
// Watch events rebuild the context outside the write lock and swap it
// atomically; a failed reload retains the last valid context and emits a
// visible error.
//
// `ry_lsp::baseline_disk_reads()` is a process-global counter. Only the baseline-reload
// tests configure a baseline, so they serialize on this guard so the
// no-I/O assertion sees only its own server's reads.
// ──────────────────────────────────────────────────────────────────────────
static BASELINE_IO_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// (#45): a failed baseline reload must retain the last valid
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
fn baseline_reload_retains_context_on_corruption() {
    use ry_testkit::LspSession;
    // Serialize against the other baseline-I/O tests so the global baseline-read
    // counter is not polluted by a concurrently-running server.
    let _io_guard = BASELINE_IO_TEST_GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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
        live.request("textDocument/inlayHint", json!({
            "textDocument": {"uri": main_uri},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
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

        // Baseline contract: a failed reload retains the last valid context.
        // The cached baseline is kept when the corrupt file fails to reload.
        assert!(
            !codes3.contains(&"RY002"),
            "phase 3: RY002 must still be suppressed (last valid baseline retained); got: {codes3:?}"
        );
    });
}

/// (#45): a *successful* baseline reload must converge to the new
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
fn baseline_reload_converges_to_new_value() {
    use ry_testkit::LspSession;
    // Serialize against the other baseline-I/O tests so the global baseline-read
    // counter is not polluted by a concurrently-running server.
    let _io_guard = BASELINE_IO_TEST_GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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
        // Sync barrier: the inlayHint response guarantees the watch
        // notification (and its outside-the-lock context rebuild) has been
        // processed.
        live.request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": main_uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
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

/// (#45): the publish/inlay-hint hot path performs ZERO
/// baseline file reads. `baseline_disk_reads()` counts every disk read by
/// the context loader; a publish that touches it betrays a regression.
#[test]
fn publish_path_performs_no_baseline_disk_io() {
    use ry_testkit::LspSession;
    // Serialize against the other baseline-I/O tests so the global baseline-read
    // counter is not polluted by a concurrently-running server.
    let _io_guard = BASELINE_IO_TEST_GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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

        // Exercise inlay hints (read path) and publish (diagnostic path).
        live.request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": main_uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
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
            "publish/inlay-hint path must perform zero baseline disk reads; got {} extra read(s)",
            reads_after.saturating_sub(reads_before)
        );
    });
}

// ──────────────────────────────────────────────────────────────────────────
// Invalid root ry.toml degrades entirely to defaults
//
// Pinned semantics: a root `ry.toml` that parses as TOML but fails
// `Config::validate` (here `[index] max-files = 0`, which validate
// rejects with "index.max-files") degrades the root config channel to
// `Config::default()` plus EMPTY user stubs — one warn, never a fatal
// error, and never a half-applied config. The diet dropped a lenient
// stubs-only parser that re-parsed the broken file and kept applying its
// `typeshed` entries; this test locks the full-defaults behavior in.
// ──────────────────────────────────────────────────────────────────────────

/// An invalid root `ry.toml` degrades ENTIRELY to defaults: initialize
/// still succeeds, diagnostics still publish, and nothing from the broken
/// file applies — not its `ignore`/`error` severity entries (default
/// filter and default severity hold) and not its `typeshed` stubs (empty
/// user-stub set). The probe document lives OUTSIDE every workspace
/// folder, so its check flows through the root-level
/// `load_root_config_and_stubs` channel (`state.user_stubs` and the root
/// filter) — the exact path whose Err branch degrades to defaults.
///
/// A control session against an identical-but-valid root config proves
/// the RY001 probe is live: with the stubs applied, `my_func()` returns
/// character and `if (my_func())` fires RY001. Without that contrast, the
/// RY001-absence assertion below could pass vacuously.
#[test]
fn invalid_root_rytoml_degrades_entirely_to_defaults() {
    let fixture = FixtureProject::empty().unwrap();
    // Top-level keys precede `[index]` so the file deserializes fully as
    // a `Config` and is rejected by `validate()` ("index.max-files"), not
    // by the TOML or schema parse. `ignore`/`error`/`typeshed` are
    // half-apply probes: none of them may take effect.
    fixture
        .write_file(
            "broken/ry.toml",
            "ignore = [\"RY002\"]\nerror = [\"RY002\"]\ntypeshed = [\"stubs\"]\n\n[index]\nmax-files = 0\n",
        )
        .unwrap();
    // Control root: identical stubs, no validation error.
    fixture
        .write_file("valid/ry.toml", "typeshed = [\"stubs\"]\n")
        .unwrap();
    let stub = serde_json::to_string(&json!({
        "schema_version": "1",
        "package": "localdep",
        "version": "test",
        "functions": {
            "my_func": {
                "params": [],
                "return": {"mode": "character", "length": "1"}
            }
        }
    }))
    .unwrap();
    fixture
        .write_file("broken/stubs/localdep.json", &stub)
        .unwrap();
    fixture
        .write_file("valid/stubs/localdep.json", &stub)
        .unwrap();

    // Probe document. Line 1 pins the default filter and severity (the
    // broken config would both ignore and error RY002). Lines 2-3 pin the
    // stub channel: applied stubs give `my_func` a character return type,
    // firing RY001 on line 3.
    let probe = "if (c(TRUE, FALSE)) print(1)\nlibrary(localdep)\nif (my_func()) print(1)\n";
    fixture.write_file("outside/main.R", probe).unwrap();

    let broken_root = fixture.path("broken");
    let valid_root = fixture.path("valid");
    let doc_uri = file_uri(&fixture.path("outside/main.R"));

    // Start a session on `root`, open the workspace-unowned probe
    // document, and return the published (code, severity) pairs.
    // Initialize must succeed and return capabilities even when the root
    // `ry.toml` is invalid — degradation must not fail the session.
    async fn open_unowned_and_collect(root: &Path, uri: &str, text: &str) -> Vec<(String, u64)> {
        use ry_testkit::LspSession;

        let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server =
            tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
        let mut live = LspSession::new(client_reader, client_writer);
        let init = live
            .initialize(root)
            .await
            .expect("initialize must succeed despite an invalid root ry.toml");
        assert!(
            init.get("capabilities").is_some(),
            "initialize must return capabilities despite an invalid root ry.toml"
        );

        let mark = live.publication_mark();
        live.open(uri, 1, text).await.unwrap();
        let publish = live.published_diagnostics_after(uri, mark).await.unwrap();
        let pairs = publish["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                (
                    d["code"].as_str().unwrap_or("").to_string(),
                    d["severity"].as_u64().unwrap_or(0),
                )
            })
            .collect();

        let _ = live.shutdown().await;
        drop(live);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
        pairs
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // Control: a valid root config applies the stubs, so `my_func()`
        // resolves to a character return and RY001 fires. This proves the
        // probe distinguishes stubs-applied from stubs-absent.
        let valid_pairs = open_unowned_and_collect(&valid_root, &doc_uri, probe).await;
        assert!(
            valid_pairs.iter().any(|(code, _)| code == "RY001"),
            "control: valid root config must apply the stubs and fire RY001; got {valid_pairs:?}"
        );
        assert!(
            valid_pairs.iter().any(|(code, _)| code == "RY002"),
            "control: RY002 must fire under the valid root config; got {valid_pairs:?}"
        );

        // Invalid root config: full-defaults degradation.
        let broken_pairs = open_unowned_and_collect(&broken_root, &doc_uri, probe).await;
        // Diagnostics still publish for the unowned document.
        assert!(
            !broken_pairs.is_empty(),
            "diagnostics must still publish under the invalid root config"
        );
        // Default filter holds: the broken config's `ignore = ["RY002"]`
        // did not half-apply, so RY002 still fires.
        assert!(
            broken_pairs.iter().any(|(code, _)| code == "RY002"),
            "default filter must hold: RY002 must survive the broken config's ignore entry; got {broken_pairs:?}"
        );
        // Default severity holds: the broken config's `error = ["RY002"]`
        // did not half-apply, so RY002 stays at its default Warning (2),
        // not Error (1).
        let ry002_severities: Vec<u64> = broken_pairs
            .iter()
            .filter(|(code, _)| code == "RY002")
            .map(|(_, severity)| *severity)
            .collect();
        assert!(
            !ry002_severities.is_empty() && ry002_severities.iter().all(|s| *s == 2),
            "default severity must hold: RY002 must publish as Warning(2), not Error(1); got {broken_pairs:?}"
        );
        // Empty stubs hold: the broken config's `typeshed = ["stubs"]`
        // did not half-apply, so `my_func` has no stub return type and
        // RY001 must be absent (the control shows it fires when applied).
        assert!(
            !broken_pairs.iter().any(|(code, _)| code == "RY001"),
            "no typeshed stubs from the broken config: RY001 must be absent; got {broken_pairs:?}"
        );
    });
}

// ──────────────────────────────────────────────────────────────────────────
// Precompute filters once per folder (#46)
//
// Filter, confidence, and exclude values are compiled once while building
// each `FolderAnalysisContext` and borrowed inside the per-file publish
// loop — visible structurally in `build_folder_contexts`, which computes
// them per folder rather than per published file.
// ──────────────────────────────────────────────────────────────────────────

/// #46: with many files in one folder, every published diagnostic must
/// still be byte-for-byte correct — the publish loop borrows the
/// per-folder precomputed filter/confidence/excludes instead of
/// recompiling anything per file. This test creates many files in one
/// folder, opens a trigger document, and checks the first and last
/// indexed files publish identical, correct diagnostics.
#[test]
fn many_files_flat_filter_construction() {
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
        let server =
            tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
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

        // Collect diagnostics for the first and last indexed files from
        // the background index's publications. The background index +
        // debounced publish will send diagnostics for trigger.R and all
        // indexed files; we keep receiving until both sentinel files have
        // published.
        let first_uri = file_uri(&fixture.path("file_00.R"));
        let last_uri = file_uri(&fixture.path("file_31.R"));
        let mut first_diags: Vec<Published> = Vec::new();
        let mut last_diags: Vec<Published> = Vec::new();

        // Drain until diagnostics for both sentinel files have been
        // published, bounded by a total deadline. Quiescence (one silent
        // 500 ms window) cannot decide completion here: on a loaded
        // machine the background index of 32 files plus the publish
        // debounce can pause longer than a single window before the first
        // indexed-file publication, which used to end collection with both
        // sentinel vectors still empty and fail the assertions below with
        // no production defect (#90).
        let drain_deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while (first_diags.is_empty() || last_diags.is_empty())
            && tokio::time::Instant::now() < drain_deadline
        {
            match tokio::time::timeout_at(drain_deadline, client.receive()).await {
                Ok(Ok(message)) => {
                    if message.get("method") == Some(&json!("textDocument/publishDiagnostics")) {
                        if message.pointer("/params/uri") == Some(&json!(first_uri)) {
                            first_diags = published_from_lsp(
                                &message,
                                &fixture.path("file_00.R"),
                                fixture.root(),
                            );
                        }
                        if message.pointer("/params/uri") == Some(&json!(last_uri)) {
                            last_diags = published_from_lsp(
                                &message,
                                &fixture.path("file_31.R"),
                                fixture.root(),
                            );
                        }
                    }
                }
                Ok(Err(e)) => panic!("transport error during drain: {e}"),
                Err(_) => break, // total deadline exhausted before both files published
            }
        }

        let shutdown_id = client.request("shutdown", Value::Null).await.unwrap();
        client.receive_until(|m| m.get("id") == Some(&json!(shutdown_id)), 128).await.unwrap();
        client.notify("exit", Value::Null).await.unwrap();
        drop(client);
        tokio::time::timeout(std::time::Duration::from_secs(5), server).await.unwrap().unwrap().unwrap();

        // Every indexed file must produce the same RY090 diagnostic.
        assert!(
            !first_diags.is_empty() && first_diags.iter().any(|d| d.code == "RY090"),
            "file_00 must produce RY090; got {:?}",
            first_diags
        );
        assert!(
            !last_diags.is_empty() && last_diags.iter().any(|d| d.code == "RY090"),
            "file_31 must produce RY090; got {:?}",
            last_diags
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
        }
        assert_eq!(first_diags[0].path, "file_00.R");
        assert_eq!(last_diags[0].path, "file_31.R");
    });
}

// ──────────────────────────────────────────────────────────────────────────
// Shared, bounded discovery (#48)
//
// Bug being pinned: the CLI (`collect_r_files`) and LSP
// (`index::discover_r_files`) used different discovery rules — for example,
// the CLI skipped `target/` while the LSP did not. The fix moved discovery
// behind a shared module so both modes agree.
// ──────────────────────────────────────────────────────────────────────────

/// (#48): the same fixture tree must produce the same discovered path
/// set in CLI and LSP. The fixture includes a `target/` directory, a hidden
/// directory (both skip), and a normal file. Every file has a diagnostic so
/// the discovery set is observable through published diagnostic paths.
#[test]
fn cli_lsp_discovery_set_equality() {
    let fixture = FixtureProject::empty().unwrap();
    // Normal file with a diagnostic.
    fixture.write_file("normal.R", "length(xx = 1L)\n").unwrap();
    // File in target/ — both modes skip this directory.
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
