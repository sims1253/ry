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
//! The server publishes the diagnostics `ry check` produces; it offers no
//! navigation features. Request handling lives in [`backend`].
//!
//! CRITICAL INVARIANT: the LSP protocol uses stdout for JSON-RPC framing.
//! Any tracing or log output that lands on stdout will corrupt the stream
//! and crash the client. All `tracing` output is routed to stderr via
//! the CLI's `tracing_subscriber` initialization before `run()` is called.

/// Test-only scheduling gates. Production code parks at a [`Seam`] only
/// after a test armed it; an unarmed seam is a single atomic swap.
///
/// Gates are thread-local: each test runs on a current-thread runtime, so
/// the server's tasks share the test's gates while parallel tests on other
/// threads stay isolated. Compiled only under the `test-util` feature.
#[cfg(feature = "test-util")]
pub mod test_seam {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    /// A point where production code can be parked or observed.
    #[derive(Clone, Copy, Debug)]
    pub enum Seam {
        /// `parsed_file` after reading text/version/tree, before parsing.
        Parse,
        /// One-shot signal after a `didChange` was applied and queued.
        DidChange,
        /// `didChange` before its document transaction.
        ChangeTransition,
        /// The index writer's full scan, between its walk and its install.
        Scan,
    }

    #[derive(Default)]
    struct Gate {
        armed: AtomicBool,
        arrived: Notify,
        release: Notify,
        landed: Notify,
    }

    thread_local! {
        static GATES: Arc<[Gate; 4]> = Arc::new(Default::default());
    }

    fn gates() -> Arc<[Gate; 4]> {
        GATES.with(Arc::clone)
    }

    /// Arm `seam`; the next production arrival consumes the arm.
    pub fn arm(seam: Seam) {
        gates()[seam as usize].armed.store(true, Ordering::Release);
    }

    /// Wait until an armed seam parked its caller.
    pub async fn wait_arrived(seam: Seam) {
        gates()[seam as usize].arrived.notified().await;
    }

    /// Resume the caller parked at `seam`.
    pub fn release(seam: Seam) {
        gates()[seam as usize].release.notify_one();
    }

    /// Wait until the parked caller resumed past its decision point, or
    /// until a one-shot signal seam fired.
    pub async fn wait_landed(seam: Seam) {
        gates()[seam as usize].landed.notified().await;
    }

    /// Park at `seam` when armed. Returns whether the caller parked.
    pub(crate) async fn pause(seam: Seam) -> bool {
        let gates = gates();
        let gate = &gates[seam as usize];
        if gate.armed.swap(false, Ordering::AcqRel) {
            gate.arrived.notify_one();
            gate.release.notified().await;
            return true;
        }
        false
    }

    /// Report that a caller parked at `seam` made its decision.
    pub(crate) fn landed(seam: Seam) {
        gates()[seam as usize].landed.notify_one();
    }

    /// Fire a one-shot signal seam when armed.
    pub(crate) fn signal(seam: Seam) {
        let gates = gates();
        let gate = &gates[seam as usize];
        if gate.armed.swap(false, Ordering::AcqRel) {
            gate.landed.notify_one();
        }
    }
}

mod backend;
mod diagnostics;
mod hints;
mod index;
mod positions;
mod settings;

use backend::Backend;
#[cfg(feature = "test-util")]
pub use backend::baseline_disk_reads;
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
    let (service, socket) = LspService::build(Backend::new).finish();
    let server = Server::new(reader, writer, socket);
    // Tests park several handlers at seams at once; tower-lsp's default of
    // four in-flight messages would leave the next notification undelivered.
    #[cfg(feature = "test-util")]
    let server = server.concurrency_level(64);
    server.serve(service).await;
    Ok(())
}

#[cfg(test)]
mod tests;
