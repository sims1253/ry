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
//! `diagnostics`, `positions`).
//!
//! CRITICAL INVARIANT: the LSP protocol uses stdout for JSON-RPC framing.
//! Any tracing or log output that lands on stdout will corrupt the stream
//! and crash the client. All `tracing` output is routed to stderr via
//! the CLI's `tracing_subscriber` initialization before `run()` is called.

/// Test-only barriers for parse/didChange interleaving and initial indexing.
/// The parse sequence is documented at the `maybe_pause` call site in
/// `backend::parsed_file`. The seams control scheduling
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
        static INITIAL_INDEX_BARRIER: Arc<InitialIndexBarrier> = Arc::new(InitialIndexBarrier::default());
    }

    #[derive(Default)]
    struct InitialIndexBarrier {
        armed: AtomicBool,
        cycle_waiting: AtomicBool,
        arrived: Notify,
        release: Notify,
        cycle_completed: Notify,
    }

    /// Hold initial indexing until a diagnostic cycle has run on an open file.
    pub fn arm_initial_index() {
        INITIAL_INDEX_BARRIER.with(|barrier| {
            barrier.armed.store(true, Ordering::Release);
            barrier.cycle_waiting.store(true, Ordering::Release);
        });
    }

    pub async fn wait_initial_index() {
        let barrier = INITIAL_INDEX_BARRIER.with(Arc::clone);
        barrier.arrived.notified().await;
    }

    pub async fn wait_initial_diagnostic_cycle() {
        let barrier = INITIAL_INDEX_BARRIER.with(Arc::clone);
        barrier.cycle_completed.notified().await;
    }

    pub fn release_initial_index() {
        INITIAL_INDEX_BARRIER.with(|barrier| barrier.release.notify_one());
    }

    pub(crate) async fn maybe_pause_initial_index() {
        let barrier = INITIAL_INDEX_BARRIER.with(Arc::clone);
        if barrier.armed.swap(false, Ordering::AcqRel) {
            barrier.arrived.notify_one();
            barrier.release.notified().await;
        }
    }

    pub(crate) fn note_initial_diagnostic_cycle() {
        INITIAL_INDEX_BARRIER.with(|barrier| {
            if barrier.cycle_waiting.swap(false, Ordering::AcqRel) {
                barrier.cycle_completed.notify_one();
            }
        });
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

    /// One-shot rendezvous gate sequencing a single index commit against
    /// the test driver so overlapping index writers interleave
    /// deterministically (#526): the first writer to arrive after arming
    /// waits for the test to release it, then signals when its commit
    /// decision is made. The arm is consumed atomically, so later writers
    /// pass through. Like the barriers above this gate is thread-local
    /// (see the module docs) and pauses only scheduling: the waiter
    /// holds no state lock while parked. Thread-locality is load-bearing —
    /// parallel tests on different threads stay isolated — so arm, wait,
    /// and release must run on the test's thread, which is the server's
    /// thread too under the `new_current_thread` runtimes every test uses.
    struct CommitGate {
        armed: AtomicBool,
        arrived: Notify,
        release: Notify,
        landed: Notify,
    }

    impl CommitGate {
        fn new() -> Self {
            Self {
                armed: AtomicBool::new(false),
                arrived: Notify::new(),
                release: Notify::new(),
                landed: Notify::new(),
            }
        }

        /// Park the first arrival after arming until the test releases
        /// it. Returns whether this writer paused: only a paused writer
        /// signals `landed`, so the test's post-release wait cannot
        /// consume a pass-through writer's signal.
        async fn maybe_pause(&self) -> bool {
            if self.armed.swap(false, Ordering::AcqRel) {
                self.arrived.notify_one();
                self.release.notified().await;
                return true;
            }
            false
        }

        /// Signal that a paused writer made its commit decision (landed
        /// or discarded). Call only when `maybe_pause` returned true.
        fn note_landed(&self) {
            self.landed.notify_one();
        }
    }

    thread_local! {
        static REFRESH_COMMIT_GATE: Arc<CommitGate> = Arc::new(CommitGate::new());
        static SCAN_COMMIT_GATE: Arc<CommitGate> = Arc::new(CommitGate::new());
        static POST_REFRESH_COMMIT_GATE: Arc<CommitGate> = Arc::new(CommitGate::new());
    }

    fn refresh_gate() -> Arc<CommitGate> {
        REFRESH_COMMIT_GATE.with(Arc::clone)
    }

    fn scan_gate() -> Arc<CommitGate> {
        SCAN_COMMIT_GATE.with(Arc::clone)
    }

    fn post_refresh_gate() -> Arc<CommitGate> {
        POST_REFRESH_COMMIT_GATE.with(Arc::clone)
    }

    /// Arm the per-file refresh gate: the next `refresh_disk_entry`
    /// commit pauses after its blocking read, before taking the state
    /// lock. The test proves the read saw pre-write bytes by writing
    /// only after `wait_refresh_commit` returns: the arrival signal is
    /// sent after the read completed, so the write is strictly later.
    pub fn arm_refresh_commit() {
        refresh_gate().armed.store(true, Ordering::Release);
    }

    /// Wait for the armed refresh to arrive at its commit point.
    pub async fn wait_refresh_commit() {
        refresh_gate().arrived.notified().await;
    }

    /// Release the paused refresh commit.
    pub fn release_refresh_commit() {
        refresh_gate().release.notify_one();
    }

    /// Wait for the released refresh to make its commit decision. The
    /// refresh's handler does not always republish (a discarded commit
    /// returns false and the caller stands down), so this rendezvous —
    /// not a publication — is the proof the commit point passed.
    pub async fn wait_refresh_landed() {
        refresh_gate().landed.notified().await;
    }

    /// Arm the full-scan gate: the next `spawn_background_index` commit
    /// pauses after its blocking walk, before taking the state lock. A
    /// scan that arrived here walked pre-save bytes whenever the test
    /// saves only after `wait_scan_commit` returns. No `landed`
    /// rendezvous: the scan's handler republishes unconditionally after
    /// the spawn returns, so its publication is the landed proof.
    pub fn arm_scan_commit() {
        scan_gate().armed.store(true, Ordering::Release);
    }

    /// Wait for the armed scan to arrive at its commit point.
    pub async fn wait_scan_commit() {
        scan_gate().arrived.notified().await;
    }

    /// Release the paused scan commit.
    pub fn release_scan_commit() {
        scan_gate().release.notify_one();
    }

    /// Called by `refresh_disk_entry` (production code) between the
    /// blocking read/parse and the commit lock. When not armed this is
    /// a no-op (a single atomic swap plus, for pass-through writers, no
    /// signal at all).
    pub(crate) async fn maybe_pause_refresh_commit() {
        let gate = refresh_gate();
        if gate.maybe_pause().await {
            gate.note_landed();
        }
    }

    /// Arm the post-commit gate: the next `refresh_disk_entry` commit
    /// pauses after its insert-and-bump critical section releases, while
    /// still observed before the caller acts on the landed result. Lets
    /// a test prove the generation bumped atomically with the insert:
    /// release the refresh, wait here, and the new generation is already
    /// visible while no caller-side bump could have run yet.
    pub fn arm_post_refresh_commit() {
        post_refresh_gate().armed.store(true, Ordering::Release);
    }

    /// Wait for the armed refresh to finish its commit critical section.
    pub async fn wait_post_refresh_commit() {
        post_refresh_gate().arrived.notified().await;
    }

    /// Release the paused post-commit refresh.
    pub fn release_post_refresh_commit() {
        post_refresh_gate().release.notify_one();
    }

    /// Called by `refresh_disk_entry` (production code) after the commit
    /// lock releases, before returning the landed verdict. No-op when
    /// not armed.
    pub(crate) async fn maybe_pause_post_refresh_commit() {
        let gate = post_refresh_gate();
        if gate.maybe_pause().await {
            gate.note_landed();
        }
    }

    /// Called by `spawn_background_index` (production code) between the
    /// blocking walk and the commit lock. Same no-op-when-unarmed
    /// contract as the refresh gate.
    pub(crate) async fn maybe_pause_scan_commit() {
        scan_gate().maybe_pause().await;
    }

    thread_local! {
        static PUBLICATION_ACK_GATE: Arc<CommitGate> = Arc::new(CommitGate::new());
        static CONTEXT_INSTALL_GATE: Arc<CommitGate> = Arc::new(CommitGate::new());
    }

    fn publication_ack_gate() -> Arc<CommitGate> {
        PUBLICATION_ACK_GATE.with(Arc::clone)
    }

    fn context_install_gate() -> Arc<CommitGate> {
        CONTEXT_INSTALL_GATE.with(Arc::clone)
    }

    /// Arm the publication-ack gate: the next landed-refresh pipeline
    /// (watched handler, `didClose`, or reconciliation-driver round)
    /// pauses AFTER its context refresh settles and the publication is
    /// scheduled, BEFORE it acknowledges the path's pending
    /// obligation. Lets a test deterministically park an OLDER
    /// refresh's completion at exactly the point where a
    /// version-unaware acknowledgement could retire a NEWER event's
    /// re-armed obligation: while parked, the test advances the newer
    /// event to its context/publication phase, then resumes the older
    /// acknowledgement and observes whether the newer entry survives.
    /// The waiter holds no state lock.
    pub fn arm_publication_ack() {
        publication_ack_gate().armed.store(true, Ordering::Release);
    }

    /// Wait for the armed pipeline to arrive at its acknowledgement
    /// point (context settled, publication scheduled, obligation not
    /// yet retired).
    pub async fn wait_publication_ack() {
        publication_ack_gate().arrived.notified().await;
    }

    /// Release the paused acknowledgement.
    pub fn release_publication_ack() {
        publication_ack_gate().release.notify_one();
    }

    /// Called by the landed-refresh pipelines (production code)
    /// between the context/publication settlement and the obligation
    /// acknowledgement, holding no lock. No-op when not armed.
    pub(crate) async fn maybe_pause_publication_ack() {
        publication_ack_gate().maybe_pause().await;
    }

    /// Arm the context-install gate: the next
    /// `refresh_one_package_context` attempt pauses AFTER its blocking
    /// resolve, with the resolved context and its generation snapshot
    /// in hand, BEFORE taking the lock whose generation guard decides
    /// the install. Lets a test land a generation-moving writer in
    /// exactly that window, so the attempt deterministically loses its
    /// generation race (the retry ladder and its full-scan backstop can
    /// then be driven the same way, through `arm_scan_commit`). The
    /// waiter holds no state lock; re-arm between releases to park a
    /// retry the same way.
    pub fn arm_context_install() {
        context_install_gate().armed.store(true, Ordering::Release);
    }

    /// Wait for the armed context attempt to arrive at its install
    /// decision point.
    pub async fn wait_context_install() {
        context_install_gate().arrived.notified().await;
    }

    /// Release the paused context install.
    pub fn release_context_install() {
        context_install_gate().release.notify_one();
    }

    /// Called by `refresh_one_package_context` (production code)
    /// between the blocking resolve and the install lock, holding no
    /// lock. No-op when not armed.
    pub(crate) async fn maybe_pause_context_install() {
        context_install_gate().maybe_pause().await;
    }

    thread_local! {
        static DRIVER_IDLE_GATE: Arc<CommitGate> = Arc::new(CommitGate::new());
    }

    fn driver_idle_gate() -> Arc<CommitGate> {
        DRIVER_IDLE_GATE.with(Arc::clone)
    }

    /// Arm the driver idle gate: the next reconciliation driver that
    /// finishes a round without dispatchable work parks BETWEEN its
    /// last per-path duty check and the lock hold that decides between
    /// idling out and continuing — the exact window in which an
    /// in-flight refresh's exit and its dispatcher's epilogue wake can
    /// land while the active flag still suppresses the wake. Lets a
    /// test interleave that exit deterministically and prove the
    /// re-check under the clearing lock keeps the driver scheduled.
    pub fn arm_driver_idle() {
        driver_idle_gate().armed.store(true, Ordering::Release);
    }

    /// Wait for the armed driver to arrive at its idle decision point.
    pub async fn wait_driver_idle() {
        driver_idle_gate().arrived.notified().await;
    }

    /// Release the parked driver idle decision.
    pub fn release_driver_idle() {
        driver_idle_gate().release.notify_one();
    }

    /// Called by `run_reconciliation` (production code) just before the
    /// idle-transition lock hold, holding no lock. No-op when unarmed.
    pub(crate) async fn maybe_pause_driver_idle() {
        driver_idle_gate().maybe_pause().await;
    }

    thread_local! {
        static RECONCILE_DRIVER_SPAWNS: std::sync::atomic::AtomicUsize =
            const { std::sync::atomic::AtomicUsize::new(0) };
        static RECONCILE_ROUNDS: std::sync::atomic::AtomicUsize =
            const { std::sync::atomic::AtomicUsize::new(0) };
        static BACKGROUND_INDEX_SPAWNS: std::sync::atomic::AtomicUsize =
            const { std::sync::atomic::AtomicUsize::new(0) };
    }

    /// Number of reconciliation driver tasks actually spawned since
    /// process start (test-util only). The wake path spawns only when
    /// no driver is active and obligations are pending, so a burst of
    /// events over a small path set must keep this far below the event
    /// count — the observable half of the driver's coalescing contract.
    pub fn reconciliation_driver_spawns() -> usize {
        RECONCILE_DRIVER_SPAWNS.with(|count| count.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Number of reconciliation driver rounds since process start
    /// (test-util only); each round re-drives every pending path once.
    pub fn reconciliation_rounds() -> usize {
        RECONCILE_ROUNDS.with(|count| count.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Number of background index passes actually spawned (their walk
    /// attempted) since process start (test-util only): the initial
    /// index, config/resolution-triggered rescans, initial-pass
    /// respawns, and every refresh/context ladder escalation to the
    /// full-scan backstop. The paced driver's no-spin contract is
    /// observable through it — while completion remains impossible the
    /// count must stay frozen (a parked driver scans nothing), and a
    /// stall episode may escalate at most one scan per driven round,
    /// never one per retry.
    pub fn background_index_spawns() -> usize {
        BACKGROUND_INDEX_SPAWNS.with(|count| count.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub(crate) fn note_reconciliation_driver_spawn() {
        RECONCILE_DRIVER_SPAWNS.with(|count| {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
    }

    pub(crate) fn note_reconciliation_round() {
        RECONCILE_ROUNDS.with(|count| {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
    }

    pub(crate) fn note_background_index_spawn() {
        BACKGROUND_INDEX_SPAWNS.with(|count| {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
    }
}

mod backend;
mod diagnostics;
mod hints;
mod index;
mod positions;
mod settings;

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
    let server = Server::new(reader, writer, socket);
    // Test-util builds widen tower-lsp's default of four in-flight
    // messages: the reconciliation interleave tests park several
    // watched-file handlers at their commit gates for the whole
    // interleave (the parked tail is what keeps a bystander's
    // retirement out of the driver's round window), and a fifth
    // notification queued behind four parked handlers would never
    // dispatch — a transport artifact, not the behavior under test.
    // Production keeps the library default; editors do not wait for
    // handler completion either way, and the concurrency the server
    // must tolerate (#538, #526) is unchanged.
    #[cfg(feature = "test-util")]
    let server = server.concurrency_level(64);
    server.serve(service).await;
    Ok(())
}

#[cfg(test)]
mod tests;
