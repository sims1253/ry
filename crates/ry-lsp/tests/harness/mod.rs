//! Shared session harness for the LSP property tests.
//!
//! One copy of the client-session type, the source variants the
//! properties edit between, and the spawn/quiesce plumbing, used by both
//! `session.rs` (transcript + convergence property) and
//! `session_state_machine.rs` (extended operation alphabet).

// Each test binary uses a different subset of this module.
#![allow(dead_code)]

use ry_testkit::LspSession;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

pub type ClientSession = LspSession<
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
>;

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
