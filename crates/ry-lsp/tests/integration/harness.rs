//! Session plumbing and the CLI/LSP comparison helpers the LSP integration
//! tests share. `server_replay.rs` includes this module too.

use ry_testkit::LspSession;
use ry_testkit::{ObservedPosition, PositionEncoding, normalize_path, normalize_position};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

pub type ClientSession = LspSession<
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
>;

pub type Server = tokio::task::JoinHandle<ExitCode>;

// ──────────────────────────────────────────────────────────────────────────
// Session plumbing
// ──────────────────────────────────────────────────────────────────────────

/// Sort diagnostics for order-independent comparison.
pub fn sorted_diagnostics(diagnostics: &[Value]) -> Vec<Value> {
    let mut diags: Vec<Value> = diagnostics.to_vec();
    diags.sort_by(|a, b| {
        serde_json::to_string(a)
            .unwrap_or_default()
            .cmp(&serde_json::to_string(b).unwrap_or_default())
    });
    diags
}

/// Extract and sort the diagnostics array from a publishDiagnostics
/// notification so comparison is order-independent.
pub fn normalize_diagnostics(publish: &Value) -> Vec<Value> {
    sorted_diagnostics(
        publish
            .pointer("/params/diagnostics")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )
}

/// Number of diagnostics with `code` in a publishDiagnostics notification.
pub fn count_code(publish: &Value, code: &str) -> usize {
    publish["params"]["diagnostics"]
        .as_array()
        .expect("publishDiagnostics array")
        .iter()
        .filter(|diagnostic| diagnostic["code"] == code)
        .count()
}

pub fn has_ry040(publish: &Value) -> bool {
    count_code(publish, "RY040") > 0
}

/// A live server's publication must match a fresh server's on the final
/// filesystem.
pub fn assert_converges(live: &Value, fresh: &Value, context: &str) {
    assert_eq!(
        normalize_diagnostics(live),
        normalize_diagnostics(fresh),
        "{context}: live server must match a fresh server on the final filesystem"
    );
}

/// Deliver a watched creation event for `uri`, the way the client reports
/// a file generated outside the editor.
pub async fn notify_created(session: &mut ClientSession, uri: &str) {
    notify_watched(session, uri, 1).await;
}

/// Send a `workspace/didChangeWatchedFiles` change event for `uri`.
pub async fn notify_changed(session: &mut ClientSession, uri: &str) {
    notify_watched(session, uri, 2).await;
}

/// Send a `workspace/didChangeWatchedFiles` deletion event for `uri`.
pub async fn notify_deleted(session: &mut ClientSession, uri: &str) {
    notify_watched(session, uri, 3).await;
}

async fn notify_watched(session: &mut ClientSession, uri: &str, kind: u8) {
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": uri, "type": kind}]}),
        )
        .await
        .unwrap();
}

/// Open `uri` and return its next publication.
pub async fn open_published(
    session: &mut ClientSession,
    uri: &str,
    version: i32,
    text: &str,
) -> Value {
    let mark = session.publication_mark();
    session.open(uri, version, text).await.unwrap();
    session
        .published_diagnostics_after(uri, mark)
        .await
        .unwrap()
}

/// Replace the text of `uri` and return its next publication.
pub async fn change_published(
    session: &mut ClientSession,
    uri: &str,
    version: i32,
    text: &str,
) -> Value {
    let mark = session.publication_mark();
    session
        .change(uri, version, json!([{"text": text}]))
        .await
        .unwrap();
    session
        .published_diagnostics_after(uri, mark)
        .await
        .unwrap()
}

/// Consume every queued or in-flight publication for `uri` until 500ms
/// pass without one, so a later mark sees only publications that later
/// actions cause. More than 16 publications means the server is stuck.
pub async fn drain_publications(session: &mut ClientSession, uri: &str) {
    for _ in 0..=16 {
        let mark = session.publication_mark();
        let wait = session.published_diagnostics_after(uri, mark);
        match tokio::time::timeout(Duration::from_millis(500), wait).await {
            Ok(received) => {
                received.expect("publication receive error during drain");
            }
            Err(_) => return,
        }
    }
    panic!("{uri} kept publishing after 16 drained publications");
}

/// Start a server on an in-memory duplex pair. The caller initializes it.
pub fn spawn_server() -> (ClientSession, Server) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(ry_lsp::run_with(server_reader, server_writer));
    (LspSession::new(client_reader, client_writer), server)
}

/// Spawn a server and initialize it with `roots` as workspace folders (the
/// first is also the root URI). Tests using the pull path answer the
/// server's `workspace/configuration` request themselves.
pub async fn spawn_session(
    roots: &[&Path],
    capabilities: Value,
    initialization_options: Option<Value>,
) -> (ClientSession, Server) {
    let (mut session, server) = spawn_server();
    let ws_folders: Vec<Value> = roots
        .iter()
        .map(|r| {
            json!({
                "uri": file_uri(r),
                "name": r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "fixture".to_string())
            })
        })
        .collect();
    let mut params = json!({
        "processId": null,
        "rootUri": file_uri(roots[0]),
        "capabilities": capabilities,
        "workspaceFolders": ws_folders
    });
    if let Some(options) = initialization_options {
        params["initializationOptions"] = options;
    }
    session.request("initialize", params).await.unwrap();
    session.notify("initialized", json!({})).await.unwrap();
    (session, server)
}

/// Shut down a session and require the server to exit successfully
/// within 3s.
pub async fn join_session(mut session: ClientSession, server: Server) {
    session.shutdown().await.expect("clean shutdown");
    drop(session);
    let code = tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .expect("server stops after exit")
        .expect("server task completes");
    assert_eq!(code, ExitCode::SUCCESS, "server exit code");
}

pub fn watching_capabilities() -> Value {
    json!({"workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}})
}

/// Answer the `client/registerCapability` request a watching client must
/// receive, returning the registered watchers.
pub async fn answer_watcher_registration(session: &mut ClientSession) -> Vec<Value> {
    let request = session
        .respond_to_request("client/registerCapability", Value::Null)
        .await
        .expect("a watching session must request watcher registration");
    request["params"]["registrations"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|registration| {
            registration["registerOptions"]["watchers"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

/// Spawn a session advertising dynamic watched-file registration and
/// answer the registration request.
pub async fn spawn_watching_session(roots: &[&Path]) -> (ClientSession, Server) {
    let (mut session, server) = spawn_session(roots, watching_capabilities(), None).await;
    answer_watcher_registration(&mut session).await;
    (session, server)
}

/// Synchronization barrier: a request/response round-trip that drains
/// leftover publications so the next `publication_mark` captures only
/// future arrivals. See `LspSession::publication_mark`.
pub async fn sync_barrier(session: &mut ClientSession, uri: &str) {
    let _ = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
            }),
        )
        .await;
}

// ──────────────────────────────────────────────────────────────────────────
// CLI/LSP comparison
// ──────────────────────────────────────────────────────────────────────────

/// A diagnostic normalized to file-relative coordinates so CLI and LSP
/// output can be compared item by item.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Published {
    pub path: String,
    pub code: String,
    pub severity: String,
    pub message: String,
    pub line: u32,
    pub byte_column: u32,
}

/// The workspace root (this crate's parent's parent), where `target/`
/// and the cargo invocations live.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Build the production `ry` binary once per test process and return the
/// path Cargo reports for it.
///
/// The path comes from Cargo's JSON artifact messages rather than a
/// hardcoded `target/debug/ry`, so it stays correct when artifacts land
/// elsewhere: a custom `CARGO_TARGET_DIR` (relative or absolute), a
/// `[build] target-dir` in a Cargo config, or a platform executable
/// suffix (`ry.exe` on Windows).
pub fn ry_binary() -> PathBuf {
    static BINARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BINARY
        .get_or_init(|| {
            let output = Command::new(env!("CARGO"))
                .current_dir(workspace_root())
                .args(["build", "--quiet", "-p", "ry-cli", "--message-format=json"])
                .output()
                .expect("build the production ry binary for the protocol gate");
            assert!(
                output.status.success(),
                "building the production ry binary failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            ry_executable_from_cargo_json(&output.stdout)
                .expect("cargo must report the ry binary artifact")
        })
        .clone()
}

/// Extract the `ry` executable's path from `cargo build
/// --message-format=json` stdout. Dependency artifacts report a null
/// `executable` and are skipped. A relative path is anchored at the
/// workspace root, where the build ran, not at the test's working
/// directory.
fn ry_executable_from_cargo_json(stdout: &[u8]) -> Option<PathBuf> {
    let stdout = std::str::from_utf8(stdout).ok()?;
    let artifact = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|msg| {
            msg["reason"] == "compiler-artifact"
                && msg["target"]["name"] == "ry"
                && msg["target"]["kind"][0] == "bin"
                && msg["executable"].is_string()
        })?;
    let path = PathBuf::from(artifact["executable"].as_str()?);
    Some(if path.is_absolute() {
        path
    } else {
        workspace_root().join(path)
    })
}

/// Percent-encoded `file://` URI for `path` (the testkit encoder, which
/// the server itself accepts).
pub fn file_uri(path: &Path) -> String {
    ry_testkit::file_uri(path).unwrap()
}

/// Convert one `ry check --output-format json` entry into a `Published`
/// with byte columns, relative to `root`.
pub fn published_from_cli_value(value: &Value, root: &Path) -> Published {
    let path = value["path"].as_str().unwrap();
    let relative = normalize_path(Path::new(path), root);
    let relative = relative.strip_prefix("./").unwrap_or(&relative).to_string();
    // Lossy, because a finding can sit in a file that is not UTF-8.
    let source = std::fs::read(root.join(&relative))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let scalar = ObservedPosition {
        line: value["line"].as_u64().unwrap() as u32 - 1,
        character: value["column"].as_u64().unwrap() as u32 - 1,
        encoding: PositionEncoding::UnicodeScalar,
    };
    let position = normalize_position(&source, &scalar).unwrap();
    Published {
        path: relative,
        code: value["code"].as_str().unwrap_or("").to_string(),
        severity: value["severity"].as_str().unwrap_or("").to_string(),
        message: value["message"].as_str().unwrap_or("").to_string(),
        line: position.line,
        byte_column: position.character,
    }
}

/// Run `ry check --output-format json <extra> <target>` in `root` and return
/// the sorted findings relative to `root`.
pub fn cli_diagnostics(
    root: &Path,
    target: impl AsRef<std::ffi::OsStr>,
    extra: &[&str],
) -> Vec<Published> {
    let output = Command::new(ry_binary())
        .current_dir(root)
        .args(["check", "--output-format", "json"])
        .args(extra)
        .arg(target)
        .output()
        .expect("run ry check");
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values: Vec<Value> = serde_json::from_slice(&output.stdout).expect("CLI JSON output");
    let mut diagnostics: Vec<_> = values
        .iter()
        .map(|value| published_from_cli_value(value, root))
        .collect();
    diagnostics.sort();
    diagnostics
}

/// Normalize an LSP `publishDiagnostics` message into sorted `Published`
/// entries so comparison is order-independent.
pub fn published_from_lsp(message: &Value, path: &Path, root: &Path) -> Vec<Published> {
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
