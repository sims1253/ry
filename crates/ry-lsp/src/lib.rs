#![allow(clippy::collapsible_if)]

//! ry language server. Publishes diagnostics for R files.
//!
//! This crate is a v1 LSP server built on top of `tower-lsp`. It supports:
//!   * `initialize` / `initialized` handshake
//!   * `textDocument/didOpen` (publishes diagnostics)
//!   * `textDocument/didChange` (incremental edits re-check and republish)
//!   * `textDocument/didClose` (clears diagnostics)
//!   * Document diagnostics via `textDocument/publishDiagnostics`
//!   * `textDocument/completion`, `signatureHelp`, `inlayHint`,
//!     `codeAction`
//!   * Graceful shutdown via `shutdown` / `exit`
//!
//! The interactive requests (`completion`, `signatureHelp`) are scoped to
//! **open documents**; none consults an unopened file on disk. The server's
//! purpose is the diagnostics `ry check` produces; whole-workspace
//! navigation over unopened files was removed because it resolved symbols
//! by spelling rather than by binding, and the outline/navigation features
//! built on the same spelling-match identity were removed with it (the
//! `hover`, `definition`, `references`, `documentSymbol`, and
//! `workspace/symbol` capabilities — see issue #87).
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

/// P36-W4 (#53): Test-only scheduler/barrier seam for forcing
/// parse/didChange interleaving. The seam controls scheduling only; cache
/// policy (version-stamped tree rejection) is production code in
/// `backend::parsed_file` and `State::store_tree`/`State::tree_for`.
///
/// The barrier is **thread-local**: each test creates a single-threaded
/// (`new_current_thread`) tokio runtime, so the server and its spawned
/// tasks share the barrier with the test, while other tests running in
/// parallel on different threads are completely isolated. No test sleeps;
/// the barrier uses `tokio::sync::Notify` for deterministic rendezvous.
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

    /// Arm the barrier so the next `parsed_file` cache miss pauses after
    /// reading the document text/version/tree but before parsing. Also
    /// arms the `didChange` notification so the test can confirm the new
    /// version is installed before releasing the paused parse. Both flags
    /// are consumed atomically (once each); subsequent calls proceed
    /// normally.
    pub fn arm() {
        let b = barrier();
        b.armed.store(true, Ordering::Release);
        b.did_change_waiting.store(true, Ordering::Release);
    }

    /// Wait for `parsed_file` to arrive at the barrier. Returns once the
    /// server-side parse has read the document but has not yet parsed it,
    /// so the test can install a new version before the parse completes.
    pub async fn wait_arrived() {
        barrier().arrived.notified().await;
    }

    /// Wait for a `didChange` to be fully processed: document updated,
    /// version bumped, and diagnostics re-scheduled. The test calls this
    /// after sending `didChange` and before releasing the barrier, so the
    /// version-stamped cache rejection is exercised deterministically.
    pub async fn wait_did_change() {
        barrier().did_change_fired.notified().await;
    }

    /// Release the paused parse so it can finish. The stale result is
    /// rejected by the version-stamped tree cache; the retry loop in
    /// `parsed_file` then parses the current version fresh.
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
    /// (via `arm`). The flag is consumed atomically so only the first
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
// P36-W5 (#45): re-export the baseline disk-read counter so integration
// tests can assert that the publish/inlay-hint/completion hot path performs
// no baseline file I/O.
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
    // Delegates to `run_with_counters` and discards the handle, so production
    // and the counter-observing tests construct the server through exactly one
    // code path. Duplicating the `LspService::build` call here would let the
    // two drift, and the drift would be invisible: the tests would keep
    // passing while exercising a server the CLI never runs.
    let (server, _counters) = run_with_counters(reader, writer);
    server.await;
    Ok(())
}

/// P37-W6 (#46): Test-only handle to a single server's filter-compile
/// counters, returned by [`run_with_counters`]. Because the counters live in
/// per-server [`State`] (not process globals), each spawned server observes
/// only its own compilations, so parallel integration tests never trip each
/// other's "zero compiles during publish" assertion.
pub struct ServerCounters {
    compile_during_last_publish: Arc<std::sync::atomic::AtomicU64>,
    filter_compile_count: Arc<std::sync::atomic::AtomicU64>,
}

impl ServerCounters {
    /// Compile-count delta observed during this server's most recent
    /// `publish_diagnostics` cycle. Must be zero once filters are
    /// precomputed and borrowed in the publish loop.
    pub fn compile_during_last_publish(&self) -> u64 {
        self.compile_during_last_publish
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Total filter/glob compilations this server has performed. Exposed for
    /// symmetry; the publish-window assertion uses
    /// [`compile_during_last_publish`](Self::compile_during_last_publish).
    pub fn filter_compile_count(&self) -> u64 {
        self.filter_compile_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Test-only: like [`run_with`], but also returns a handle to this server's
/// counters so assertions are scoped to one instance rather than the process.
/// The returned future drives the server (spawn it as you would `run_with`);
/// the [`ServerCounters`] handle shares the same per-server `Arc`s, so reads
/// after the server quiesces see exactly what the server stored.
pub fn run_with_counters<I, O>(
    read: I,
    write: O,
) -> (impl std::future::Future<Output = ()>, ServerCounters)
where
    I: tokio::io::AsyncRead + Unpin,
    O: tokio::io::AsyncWrite + Unpin,
{
    let state = State::default();
    let counters = ServerCounters {
        compile_during_last_publish: Arc::clone(&state.compile_during_last_publish),
        filter_compile_count: Arc::clone(&state.filter_compile_count),
    };
    let (service, socket) = LspService::build(|client| Backend {
        client,
        state: Arc::new(Mutex::new(state)),
    })
    .finish();
    let fut = async move {
        Server::new(read, write, socket).serve(service).await;
    };
    (fut, counters)
}

#[cfg(test)]
mod tests;
