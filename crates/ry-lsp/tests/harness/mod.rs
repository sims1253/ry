//! Shared test harness for the LSP integration tests.
//!
//! Two halves, one module:
//!
//! * the session plumbing — the client-session type, the source variants
//!   the properties edit between, and the spawn/quiesce helpers — used by
//!   `session_state_machine.rs` (the extended-alphabet convergence
//!   property plus the deterministic transcript tests) and
//!   `configuration_refresh.rs`;
//! * the cross-mode protocol gates — the `Published` normalization and
//!   the CLI/LSP comparison plumbing — used by `protocol.rs`,
//!   `protocol_contract.rs`, and `testkit.rs`, so the contract stays
//!   "LSP published diagnostics equal `ry check` run independently in
//!   the same root".

// Each test binary uses a different subset of this module.
#![allow(dead_code)]

use ry_testkit::LspSession;
use ry_testkit::{ObservedPosition, PositionEncoding, normalize_path, normalize_position};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub type ClientSession = LspSession<
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
>;

// ──────────────────────────────────────────────────────────────────────────
// Session plumbing
// ──────────────────────────────────────────────────────────────────────────

/// Source variants with varying Unicode prefixes so incremental-edit ranges
/// exercise BMP, combining-mark, and astral-plane UTF-16 positions.  Each
/// diagnostic variant emits RY090 (partial argument name) and RY091 so the
/// oracle comparison is meaningful.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SourceVariant {
    Clean,
    AsciiDiagnostic,
    BmpDiagnostic,
    AstralDiagnostic,
}

impl SourceVariant {
    pub fn text(self) -> &'static str {
        match self {
            Self::Clean => "x <- 1L\ny <- 2L\n",
            Self::AsciiDiagnostic => "z <- length(xx = 1L)\ny <- 2L\n",
            Self::BmpDiagnostic => "café <- length(xx = 1L)\ny <- 2L\n",
            Self::AstralDiagnostic => "\u{1f600} <- length(xx = 1L)\ny <- 2L\n",
        }
    }

    /// The content of the first line without the trailing newline.  Used as
    /// the replacement text for an incremental range edit targeting line 0.
    pub fn first_line(self) -> &'static str {
        self.text().lines().next().unwrap()
    }
}

/// Compute the UTF-16 code-unit length of the first line of `text`.  This is
/// the LSP character column at the end of line 0.
pub fn first_line_utf16_len(text: &str) -> u32 {
    let end = text.find('\n').unwrap_or(text.len());
    text[..end].encode_utf16().count() as u32
}

/// Splice the first-line replacement, matching the server's incremental
/// range-to-byte conversion: replace everything from (0, 0) to
/// (0, first_line_utf16_len) with the new first line, keeping the rest.
pub fn apply_incremental_edit(old: &str, source: SourceVariant) -> String {
    let first_line_end_byte = old.find('\n').unwrap_or(old.len());
    let mut result = String::with_capacity(old.len() + source.first_line().len());
    result.push_str(source.first_line());
    result.push_str(&old[first_line_end_byte..]);
    result
}

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

/// Spawn a fresh server on a duplex pair and initialize the client session
/// with the given workspace folders (first folder is the root URI).
pub async fn spawn_session(roots: &[&Path]) -> (ClientSession, tokio::task::JoinHandle<()>) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move {
        let _ = ry_lsp::run_with(server_reader, server_writer).await;
    });
    let mut session = LspSession::new(client_reader, client_writer);
    let root_uri = ry_testkit::file_uri(roots[0]).unwrap();
    let ws_folders: Vec<Value> = roots
        .iter()
        .map(|r| {
            json!({
                "uri": ry_testkit::file_uri(r).unwrap(),
                "name": r.file_name().unwrap().to_string_lossy()
            })
        })
        .collect();
    session
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "workspaceFolders": ws_folders
            }),
        )
        .await
        .unwrap();
    session.notify("initialized", json!({})).await.unwrap();
    (session, server)
}

/// Spawn an LSP server over a single root with custom client
/// `capabilities` and, unless it is `Value::Null`, `initialization_options`.
/// Tests using the pull path answer the server's `workspace/configuration`
/// request themselves right after this returns. No settle wait for the
/// background indexer: it never publishes diagnostics itself (its caller
/// republishes, and no document is open here), and open documents shadow
/// disk files, so each test's `published_diagnostics_after` await (which
/// has its own timeout) is the only synchronization needed.
pub async fn spawn_session_with_options(
    root: &Path,
    capabilities: Value,
    initialization_options: Value,
) -> (ClientSession, tokio::task::JoinHandle<()>) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move {
        let _ = ry_lsp::run_with(server_reader, server_writer).await;
    });
    let mut session = LspSession::new(client_reader, client_writer);
    let root_uri = ry_testkit::file_uri(root).unwrap();
    let mut params = json!({
        "processId": null,
        "rootUri": root_uri,
        "capabilities": capabilities,
        "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
    });
    if initialization_options != Value::Null {
        params["initializationOptions"] = initialization_options;
    }
    session.request("initialize", params).await.unwrap();
    session.notify("initialized", json!({})).await.unwrap();
    (session, server)
}

/// Shut down a session and bounded-join its server.
pub async fn join_session(mut session: ClientSession, server: tokio::task::JoinHandle<()>) {
    let _ = session.shutdown().await;
    drop(session);
    let _ = tokio::time::timeout(Duration::from_secs(3), server).await;
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
// Cross-mode protocol gates
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
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Build the production `ry` binary once per test process and return its
/// debug path.
pub fn ry_binary() -> PathBuf {
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
    let source = std::fs::read_to_string(root.join(&relative)).unwrap_or_default();
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
