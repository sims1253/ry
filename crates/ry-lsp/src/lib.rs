//! ry language server. Publishes diagnostics for R files.
//!
//! This crate is a v1 LSP server built on top of `tower-lsp`. It supports:
//!   * `initialize` / `initialized` handshake
//!   * `textDocument/didOpen` (publishes diagnostics)
//!   * `textDocument/didChange` (incremental edits re-check and republish)
//!   * `textDocument/didClose` (clears diagnostics)
//!   * Document diagnostics via `textDocument/publishDiagnostics`
//!   * `textDocument/hover` (type at cursor)
//!   * `textDocument/definition` (go-to-definition for variables/functions)
//!   * `textDocument/references` (find all usages of a symbol across open files)
//!   * `textDocument/documentSymbol` (outline view of the file's bindings)
//!   * `workspace/symbol` (search for symbols across all open files)
//!   * `textDocument/rename` (workspace-wide rename of a variable / function)
//!   * `textDocument/prepareRename` (validates the cursor is on a renameable identifier)
//!   * `textDocument/completion`, `signatureHelp`, `inlayHint`,
//!     `foldingRange`, `codeAction`, `selectionRange`, `documentHighlight`
//!   * Graceful shutdown via `shutdown` / `exit`
//!
//! Architecture (PLAN Phase E3): this file is intentionally small --
//! module declarations + the `run()` entry point. All request-handler
//! logic lives in [`backend`] (`Backend`, `State`, the
//! `LanguageServer` impl, and the parse/scope/debounce caches); the
//! per-feature helpers live in their own modules (`navigation`,
//! `symbols`, `hints`, `folding`, `selection`, `diagnostics`, `ident`).
//!
//! CRITICAL INVARIANT: the LSP protocol uses stdout for JSON-RPC framing.
//! Any tracing or log output that lands on stdout will corrupt the stream
//! and crash the client. All `tracing` output is routed to stderr via
//! the CLI's `tracing_subscriber` initialization before `run()` is called.

mod backend;
mod diagnostics;
mod folding;
mod hints;
mod ident;
mod navigation;
mod selection;
mod symbols;
mod util;

use backend::{Backend, State};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::{LspService, Server};

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
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}

#[cfg(test)]
mod tests;
