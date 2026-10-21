//! ry language server. Publishes the diagnostics `ry check` produces and
//! offers inlay hints and suppression quick fixes. It has no navigation
//! features. Request handling lives in [`backend`].
//!
//! The protocol uses stdout for JSON-RPC framing, so log output on stdout
//! corrupts the stream. The CLI routes all `tracing` output to stderr
//! before it calls [`run`].

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
use std::future::Future;
use std::pin::Pin;
use std::process::ExitCode;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::ReadBuf;
use tokio::sync::oneshot;
use tower_lsp::jsonrpc::Request;
use tower_lsp::{LspService, Server};
use tower_service::Service;

/// Serve the protocol over stdin and stdout. The caller must route
/// `tracing` output to stderr first.
pub async fn run() -> ExitCode {
    run_with(tokio::io::stdin(), tokio::io::stdout()).await
}

/// Serve the protocol over caller-provided streams. Integration tests use
/// in-memory streams instead of a subprocess.
///
/// Returns once the client sends `exit` or closes the input stream. Per the
/// LSP spec, `exit` without a prior `shutdown` is a failure.
pub async fn run_with<R, W>(reader: R, writer: W) -> ExitCode
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let (service, socket) = LspService::build(Backend::new).finish();
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
