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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Notify;

    /// A point where production code can be parked or observed.
    #[derive(Clone, Copy, Debug)]
    pub enum Seam {
        /// `parsed_file` after reading text/version/tree, before parsing.
        Parse,
        /// One-shot signal after a `didChange` was applied and scheduled.
        DidChange,
        /// `didChange` before its document transaction.
        ChangeTransition,
        /// The initial background index, before its walk.
        InitialIndex,
        /// One-shot signal after a debounced diagnostics pass published.
        DiagnosticCycle,
        /// A per-file disk refresh between its read and its commit lock.
        RefreshCommit,
        /// A per-file disk refresh after its commit lock released.
        PostRefreshCommit,
        /// A full scan between its walk and its commit lock.
        ScanCommit,
        /// A landed-refresh pipeline before acknowledging its obligation.
        PublicationAck,
        /// A package-context refresh between resolve and install.
        ContextInstall,
        /// The reconciliation driver before deciding to idle.
        DriverIdle,
    }

    /// Instrumented event counts since thread start.
    #[derive(Clone, Copy, Debug)]
    pub enum Counter {
        ReconciliationDriverSpawns,
        ReconciliationRounds,
        BackgroundIndexSpawns,
    }

    #[derive(Default)]
    struct Gate {
        armed: AtomicBool,
        arrived: Notify,
        release: Notify,
        landed: Notify,
    }

    thread_local! {
        static GATES: Arc<[Gate; 11]> = Arc::new(Default::default());
        static COUNTERS: [AtomicUsize; 3] = Default::default();
    }

    fn gates() -> Arc<[Gate; 11]> {
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

    pub fn count(counter: Counter) -> usize {
        COUNTERS.with(|counters| counters[counter as usize].load(Ordering::Relaxed))
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

    /// Park at `seam` when armed and report landing once resumed.
    pub(crate) async fn pause_and_land(seam: Seam) -> bool {
        let paused = pause(seam).await;
        if paused {
            landed(seam);
        }
        paused
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

    pub(crate) fn bump(counter: Counter) {
        COUNTERS.with(|counters| counters[counter as usize].fetch_add(1, Ordering::Relaxed));
    }
}

mod backend;
mod diagnostics;
mod hints;
mod index;
mod positions;
mod settings;

#[cfg(feature = "test-util")]
pub use backend::baseline_disk_reads;
use backend::{Backend, State};
use std::future::Future;
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::ReadBuf;
use tokio::sync::{Mutex, oneshot};
use tower_lsp::jsonrpc::Request;
use tower_lsp::{LspService, Server};
use tower_service::Service;

/// Entry point for the LSP server. Reads from stdin, writes to stdout.
///
/// IMPORTANT: the caller (the CLI) MUST install a `tracing_subscriber`
/// that routes output to stderr BEFORE calling this function. Any log
/// output on stdout will corrupt the JSON-RPC stream and break the
/// client. See `crates/ry-cli/src/main.rs`'s `Cmd::Server` arm.
pub async fn run() -> ExitCode {
    run_with(tokio::io::stdin(), tokio::io::stdout()).await
}

/// Run the language server over caller-provided streams.
///
/// Production uses stdio through [`run`]. Integration tests use this seam with
/// in-memory duplex streams so large protocol matrices exercise the same
/// [`LspService`] without paying subprocess startup costs.
///
/// Returns once the client sends `exit` or closes the input stream. The exit
/// code follows the LSP spec: `exit` without a prior `shutdown` is a failure.
pub async fn run_with<R, W>(reader: R, writer: W) -> ExitCode
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let (service, socket) = LspService::build(|client| Backend {
        client,
        state: Arc::new(Mutex::new(State::default())),
    })
    .finish();
    let (code_tx, mut code_rx) = oneshot::channel();
    let (end_input_tx, end_input_rx) = oneshot::channel();
    let service = ExitSignal {
        inner: service,
        shutdown_requested: false,
        exit: Some((code_tx, end_input_tx)),
    };
    let reader = InputUntilExit {
        inner: reader,
        exit: Some(end_input_rx),
    };
    let server = Server::new(reader, writer, socket);
    // Tests park several handlers at seams at once; tower-lsp's default of
    // four in-flight messages would leave the next notification undelivered.
    #[cfg(feature = "test-util")]
    let server = server.concurrency_level(64);
    let serve = server.serve(service);
    tokio::pin!(serve);
    tokio::select! {
        biased;
        Ok(code) = &mut code_rx => {
            // Ending the input lets `serve` flush every queued response
            // before it returns. The bound covers a handler still awaiting
            // a reply from a client that has already exited.
            let _ = tokio::time::timeout(EXIT_DRAIN_LIMIT, &mut serve).await;
            code
        }
        () = &mut serve => code_rx.try_recv().unwrap_or(ExitCode::SUCCESS),
    }
}

const EXIT_DRAIN_LIMIT: Duration = Duration::from_secs(2);

/// Reports the `exit` notification, and whether `shutdown` preceded it, to
/// [`run_with`] and to the input stream.
struct ExitSignal<S> {
    inner: S,
    shutdown_requested: bool,
    exit: Option<(oneshot::Sender<ExitCode>, oneshot::Sender<()>)>,
}

impl<S: Service<Request>> Service<Request> for ExitSignal<S> {
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        match request.method() {
            "shutdown" => self.shutdown_requested = true,
            "exit" => {
                if let Some((code_tx, end_input_tx)) = self.exit.take() {
                    let code = if self.shutdown_requested {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    };
                    let _ = code_tx.send(code);
                    let _ = end_input_tx.send(());
                }
            }
            _ => {}
        }
        self.inner.call(request)
    }
}

/// Reads the client's input until `exit`, then reports end of stream.
///
/// tower-lsp only notices `exit` when the next message arrives, and a client
/// may keep the stream open after sending it; ending the input here lets the
/// serve loop wind down without waiting on the client.
struct InputUntilExit<R> {
    inner: R,
    exit: Option<oneshot::Receiver<()>>,
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for InputUntilExit<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let Some(exit) = self.exit.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        if Pin::new(exit).poll(cx).is_ready() {
            self.exit = None;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(test)]
mod tests;
