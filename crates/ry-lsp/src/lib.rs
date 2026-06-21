//! ry language server. Publishes diagnostics for R files.
//!
//! This is a v1 LSP server built on top of `tower-lsp`. It supports:
//!   * `initialize` / `initialized` handshake
//!   * `textDocument/didOpen` (publishes diagnostics)
//!   * `textDocument/didChange` (incremental edits re-check and republish)
//!   * `textDocument/didClose` (clears diagnostics)
//!   * Document diagnostics via `textDocument/publishDiagnostics`
//!   * Graceful shutdown via `shutdown` / `exit`
//!
//! Out of scope for v1: code actions, hover, go-to-definition, formatting,
//! completion, and workspace configuration requests. We DO read `ry.toml`
//! for rule severities in future revisions, but v1 ignores configuration
//! change notifications.
//!
//! To test manually:
//!   1. `echo 'Content-Length: 77\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}' | cargo run -q --bin ry -- server`
//!   2. A JSON-RPC response with the server's capabilities should come
//!      back on stdout. All tracing/logging goes to stderr so the
//!      stdout JSON-RPC stream is never corrupted.
//!
//! CRITICAL INVARIANT: the LSP protocol uses stdout for JSON-RPC framing.
//! Any tracing or log output that lands on stdout will corrupt the stream
//! and crash the client. All `tracing` output is routed to stderr via
//! the CLI's `tracing_subscriber` initialization before `run()` is called.

use ry_checker::{Diagnostic as RyDiagnostic, Project, Severity};
use ry_core::RParser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::Diagnostic as LspDiagnostic;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    /// Open documents: path -> current source text. Keeping every open
    /// document's text lets us rebuild a multi-file `Project` on each
    /// change so cross-file resolution (function defined in `a.R`
    /// visible from `b.R` when both are open in the editor) works.
    docs: HashMap<String, String>,
    /// Workspace root, set at `initialize`. Used only for diagnostics
    /// today; future revisions may use it to discover `ry.toml`.
    #[allow(dead_code)]
    root: Option<PathBuf>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let mut state = self.state.lock().await;
        state.root = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok());
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    // v1: send the whole document on each change. This
                    // avoids the complexity of incremental range sync
                    // and matches the common "re-check on save" mode of
                    // most R users. The client sends the full text in
                    // `changes[0]`.
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "ry".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        tracing::info!("ry LSP initialized");
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let text = params.text_document.text.clone();
        self.update_doc(path, text).await;
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        // TextDocumentSyncKind::FULL means the whole new text arrives in
        // `changes[0]`. v1 does not implement incremental sync, so we
        // ignore any further entries in `content_changes`.
        if let Some(change) = params.content_changes.into_iter().next() {
            self.update_doc(path, change.text).await;
            self.publish_diagnostics(uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        {
            let mut state = self.state.lock().await;
            state.docs.remove(&path);
        }
        // Clear diagnostics for the closed document so stale squiggles
        // don't linger after the user closes the file.
        self.client
            .publish_diagnostics(uri, Vec::new(), None)
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }
}

impl Backend {
    async fn update_doc(&self, path: String, text: String) {
        let mut state = self.state.lock().await;
        state.docs.insert(path, text);
    }

    /// Re-check ALL open documents and publish diagnostics for the file
    /// identified by `uri`.
    ///
    /// PERFORMANCE: for each `didChange`, we rebuild a `Project` from
    /// ALL open documents and run the full three-pass check. For small
    /// workspaces (10-50 files) this is fast enough for interactive
    /// use. For very large workspaces, the per-keystroke cost may
    /// become noticeable; a future revision should add debouncing and
    /// incremental re-checking (only re-parse the file that changed).
    async fn publish_diagnostics(&self, uri: Url) {
        // Snapshot the open docs under the lock, then drop the lock
        // before running the checker so a slow check doesn't block
        // other LSP requests (e.g. didOpen of a second file).
        let (path, docs) = {
            let state = self.state.lock().await;
            (uri_to_path(&uri), state.docs.clone())
        };

        // Build a multi-file Project from every open document so
        // cross-file calls resolve.
        let mut project = Project::new();
        for (doc_path, doc_text) in &docs {
            let mut parser = match RParser::new() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("parser init failed: {}", e);
                    continue;
                }
            };
            let file = match parser.parse(doc_path, doc_text) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("parse {}: {}", doc_path, e);
                    continue;
                }
            };
            project.add_file(doc_path.clone(), file);
        }

        let per_file = project.check();
        let diags_for_uri: Vec<LspDiagnostic> = per_file
            .into_iter()
            .filter(|(p, _)| p == &path)
            .flat_map(|(_, ds)| ds)
            .map(diagnostic_to_lsp)
            .collect();
        self.client
            .publish_diagnostics(uri, diags_for_uri, None)
            .await;
    }
}

/// Convert a `file://` URI to a filesystem path string. Falls back to
/// the URI's string form when the URI isn't a `file:` scheme (so a
/// virtual or untitled document still gets a stable key).
fn uri_to_path(uri: &Url) -> String {
    uri.to_file_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| uri.as_str().to_string())
}

/// Convert a `ry_checker::Diagnostic` to an LSP `Diagnostic`.
///
/// ry's `Span` carries pre-resolved 0-indexed `line` and 0-indexed
/// `col` (in char count). LSP positions are 0-indexed lines but
/// UTF-16 code units for the character offset. For ASCII files the two
/// are identical; for non-ASCII content this v1 conversion is an
/// approximation (we forward `col` unchanged). The end position is set
/// to a single-character range anchored at the start, which editors
/// render as a squiggle under one character. A future revision should
/// compute a precise range from the span's byte offsets.
fn diagnostic_to_lsp(d: RyDiagnostic) -> LspDiagnostic {
    let start = Position {
        line: d.span.line as u32,
        character: d.span.col as u32,
    };
    let end = Position {
        line: d.span.line as u32,
        // Single-character range so the squiggle is non-empty even for
        // zero-width spans. Future revision should use span.start/end
        // against the source text for a precise range.
        character: (d.span.col as u32) + 1,
    };
    let severity = match d.severity {
        Severity::Error => Some(DiagnosticSeverity::ERROR),
        Severity::Warning => Some(DiagnosticSeverity::WARNING),
        Severity::Info => Some(DiagnosticSeverity::INFORMATION),
    };
    LspDiagnostic {
        range: Range { start, end },
        severity,
        code: Some(NumberOrString::String(d.code.to_string())),
        source: Some("ry".to_string()),
        message: d.message,
        ..Default::default()
    }
}

/// Entry point for the LSP server. Reads from stdin, writes to stdout.
///
/// IMPORTANT: the caller (the CLI) MUST install a `tracing_subscriber`
/// that routes output to stderr BEFORE calling this function. Any log
/// output on stdout will corrupt the JSON-RPC stream and break the
/// client. See `crates/ry-cli/src/main.rs`'s `Cmd::Server` arm.
pub async fn run() -> LspResult<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::build(|client| Backend {
        client,
        state: Arc::new(Mutex::new(State::default())),
    })
    .finish();
    Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_checker::Diagnostic;
    use ry_core::Span;

    #[test]
    fn converts_error_diagnostic() {
        let d = Diagnostic::new(
            Severity::Error,
            Span::new(0, 5, 2, 4),
            "test.R",
            "RY040",
            "test message",
        );
        let lsp = diagnostic_to_lsp(d);
        assert_eq!(lsp.range.start.line, 2);
        assert_eq!(lsp.range.start.character, 4);
        // Single-character range: end character is start + 1.
        assert_eq!(lsp.range.end.character, 5);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp.message, "test message");
        assert_eq!(lsp.source.as_deref(), Some("ry"));
        match lsp.code {
            Some(NumberOrString::String(s)) => assert_eq!(s, "RY040"),
            other => panic!("expected String code, got {:?}", other),
        }
    }

    #[test]
    fn converts_warning_diagnostic() {
        let d = Diagnostic::new(
            Severity::Warning,
            Span::new(0, 5, 0, 0),
            "test.R",
            "RY001",
            "warning",
        );
        let lsp = diagnostic_to_lsp(d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn converts_info_diagnostic() {
        let d = Diagnostic::new(
            Severity::Info,
            Span::new(0, 5, 1, 2),
            "test.R",
            "RY001",
            "info",
        );
        let lsp = diagnostic_to_lsp(d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn uri_to_path_handles_file_scheme() {
        let uri = Url::parse("file:///tmp/foo.R").unwrap();
        let path = uri_to_path(&uri);
        assert!(path.ends_with("foo.R"), "path was {}", path);
    }

    #[test]
    fn uri_to_path_falls_back_for_non_file_scheme() {
        // An `untitled:` URI can't be turned into a file path; we fall
        // back to the URI string so the document still has a stable
        // identity in the open-docs map.
        let uri = Url::parse("untitled:Untitled-1").unwrap();
        let path = uri_to_path(&uri);
        assert_eq!(path, "untitled:Untitled-1");
    }
}
