//! ry language server. Publishes diagnostics for R files.
//!
//! This crate is a v1 LSP server built on top of `tower-lsp`. It supports:
//!   * `initialize` / `initialized` handshake
//!   * `textDocument/didOpen` (publishes diagnostics)
//!   * `textDocument/didChange` (incremental edits re-check and republish)
//!   * `textDocument/didClose` (clears diagnostics)
//!   * Document diagnostics via `textDocument/publishDiagnostics`
//!   * `textDocument/inlayHint`, `codeAction`
//!   * Graceful shutdown via `shutdown` / `exit`
//!
//! The server's purpose is the diagnostics `ry check` produces;
//! whole-workspace navigation over unopened files was removed because
//! it resolved symbols by spelling rather than by binding, and the
//! features built on the same spelling-match identity were removed
//! with it (the `hover`, `definition`, `references`, `documentSymbol`,
//! `workspace/symbol`, `completion`, and `signatureHelp`
//! capabilities).
//!
//! Architecture: this file is intentionally small --
//! module declarations + the `run()` entry point. All request-handler
//! logic lives in [`backend`] (`Backend`, `State`, the
//! `LanguageServer` impl, and the parse/scope/debounce caches); the
//! per-feature helpers live in their own modules (`hints`,
//! `diagnostics`, `util`).
//!
//! CRITICAL INVARIANT: the LSP protocol uses stdout for JSON-RPC framing.
//! Any tracing or log output that lands on stdout will corrupt the stream
//! and crash the client. All `tracing` output is routed to stderr via
//! the CLI's `tracing_subscriber` initialization before `run()` is called.

/// Test-only scheduler/barrier seam for forcing parse/didChange
/// interleaving (the forced sequence is documented at the `maybe_pause`
/// call site in `backend::parsed_file`). The seam controls scheduling
/// only; cache policy (version-stamped tree rejection) is production
/// code in `backend::parsed_file` and `State::store_tree`/`State::tree_for`.
///
/// Compiled only under the `test-util` feature (enabled for this
/// crate's own integration tests via the self dev-dependency), so it is
/// absent from the production `ry server` binary (#170).
///
/// The barrier is **thread-local**: each test creates a single-threaded
/// (`new_current_thread`) tokio runtime, so the server and its spawned
/// tasks share the barrier with the test, while other tests running in
/// parallel on different threads are completely isolated. No test sleeps;
/// the barrier uses `tokio::sync::Notify` for deterministic rendezvous.
#[cfg(feature = "test-util")]
pub mod test_seam {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    /// Per-thread coordination state. Stored behind an `Arc` so it can be
    /// cloned out of the `thread_local` accessor and used in async code.
    struct ParseBarrier {
        armed: AtomicBool,
        did_change_waiting: AtomicBool,
        arrived: Notify,
        release: Notify,
        did_change_fired: Notify,
    }

    impl ParseBarrier {
        fn new() -> Self {
            Self {
                armed: AtomicBool::new(false),
                did_change_waiting: AtomicBool::new(false),
                arrived: Notify::new(),
                release: Notify::new(),
                did_change_fired: Notify::new(),
            }
        }
    }

    thread_local! {
        static PARSE_BARRIER: Arc<ParseBarrier> = Arc::new(ParseBarrier::new());
    }

    /// Clone the thread-local barrier out for async use.
    fn barrier() -> Arc<ParseBarrier> {
        PARSE_BARRIER.with(|b| b.clone())
    }

    /// Arm the barrier so the next `parsed_file` cache miss pauses before
    /// parsing, and arm the `didChange`-processed notification. Both flags
    /// are consumed atomically (once each).
    pub fn arm() {
        let b = barrier();
        b.armed.store(true, Ordering::Release);
        b.did_change_waiting.store(true, Ordering::Release);
    }

    /// Wait for `parsed_file` to arrive at the barrier: it has read the
    /// document but not yet parsed it.
    pub async fn wait_arrived() {
        barrier().arrived.notified().await;
    }

    /// Wait for a `didChange` to be fully processed (document updated,
    /// version bumped, diagnostics re-scheduled); call before releasing
    /// the barrier so the version-stamped cache rejection is exercised
    /// deterministically.
    pub async fn wait_did_change() {
        barrier().did_change_fired.notified().await;
    }

    /// Release the paused parse; the version-stamped cache and the retry
    /// loop in `backend::parsed_file` handle the rest.
    pub fn release_barrier() {
        barrier().release.notify_one();
    }

    /// Called by `parsed_file` (production code). If armed, atomically
    /// disarms and pauses: signals arrival, then waits for the test to
    /// release. When not armed, this is a no-op (single relaxed load).
    pub(crate) async fn maybe_pause() {
        let b = barrier();
        if b.armed.swap(false, Ordering::AcqRel) {
            b.arrived.notify_one();
            b.release.notified().await;
        }
    }

    /// Called by `did_change` after the document is updated and diagnostics
    /// are re-scheduled. Only fires when a test has armed the notification
    /// via `arm`; the flag is consumed atomically so only the first
    /// `didChange` after arming signals.
    pub(crate) fn note_did_change() {
        let b = barrier();
        if b.did_change_waiting.swap(false, Ordering::AcqRel) {
            b.did_change_fired.notify_one();
        }
    }
}

mod backend;
mod diagnostics;
mod hints;
mod index;
mod settings;
mod util;

use backend::{Backend, State};
// Re-export the baseline disk-read counter so integration
// tests can assert that the publish/inlay-hint hot path performs
// no baseline file I/O. Test-util builds only (#170).
#[cfg(feature = "test-util")]
pub use backend::baseline_disk_reads;
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
    run_with(tokio::io::stdin(), tokio::io::stdout()).await
}

/// Run the language server over caller-provided streams.
///
/// Production uses stdio through [`run`]. Integration tests use this seam with
/// in-memory duplex streams so large protocol matrices exercise the same
/// [`LspService`] without paying subprocess startup costs.
pub async fn run_with<R, W>(reader: R, writer: W) -> LspResult<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let (service, socket) = LspService::build(|client| Backend {
        client,
        state: Arc::new(Mutex::new(State::default())),
    })
    .finish();
    Server::new(reader, writer, socket).serve(service).await;
    Ok(())
}

#[cfg(test)]
mod tests;
