//! Configuration-refresh behavior for the LSP.
//!
//! `did_change_configuration_refreshes_cached_filters` verifies that a
//! `workspace/didChangeConfiguration` raising `ry.minConfidence` changes the
//! next published diagnostics without any file edit or restart. The cached
//! per-folder severity filter, min-confidence, and excludes are recomputed
//! outside the publish loop on every configuration change; without that
//! refresh a stale cached value would persist until a filesystem rebuild or
//! server restart.

use ry_testkit::{FixtureProject, LspSession, file_uri};
use serde_json::{Value, json};
use std::path::Path;

type Session = LspSession<
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
>;

/// Spawn an LSP server and return a connected session: initialize, then
/// briefly sleep so the background indexer settles before the test drives
/// the server.
async fn spawn_session(root: &Path) -> (Session, tokio::task::JoinHandle<()>) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move {
        let _ = ry_lsp::run_with(server_reader, server_writer).await;
    });
    let mut session = LspSession::new(client_reader, client_writer);
    session.initialize(root).await.unwrap();
    // Wait for background indexing to populate disk_files.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    (session, server)
}

/// Run a future on a current-thread tokio runtime (same pattern as session.rs).
fn run<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(future)
}

/// Count occurrences of a rule code in a publishDiagnostics `diagnostics`
/// array.
fn count_code(publish: &Value, code: &str) -> usize {
    publish["params"]["diagnostics"]
        .as_array()
        .map(|diags| diags.iter().filter(|d| d["code"] == code).count())
        .unwrap_or(0)
}

// ════════════════════════════════════════════════════════════════════════════
// didChangeConfiguration refreshes the cached filter values
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn did_change_configuration_refreshes_cached_filters() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // RY090 (partial argument name) is emitted at Medium confidence by
        // default, so raising `minConfidence` to "high" suppresses it.
        fixture
            .write_file("R/diag.R", "z <- length(xx = 1L)\n")
            .unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let diag_uri = file_uri(&fixture.path("R/diag.R")).unwrap();

        // Open and capture the initial publication: RY090 must be present.
        let mark0 = session.publication_mark();
        session
            .open(&diag_uri, 1, "z <- length(xx = 1L)\n")
            .await
            .unwrap();
        let initial = session
            .published_diagnostics_after(&diag_uri, mark0)
            .await
            .unwrap();
        assert!(
            count_code(&initial, "RY090") >= 1,
            "RY090 should be present at the default confidence; got: {:?}",
            initial["params"]["diagnostics"]
        );

        // Raise minConfidence via workspace/didChangeConfiguration — no file
        // edit, no restart. Mark after the initial publication is consumed so
        // the next diag.R publish is the one triggered by this notification.
        let mark1 = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": { "ry": { "minConfidence": "high" } } }),
            )
            .await
            .unwrap();
        let after = session
            .published_diagnostics_after(&diag_uri, mark1)
            .await
            .unwrap();

        // The recomputed min_confidence must suppress RY090.
        assert_eq!(
            count_code(&after, "RY090"),
            0,
            "RY090 should be suppressed after raising minConfidence; got: {:?}",
            after["params"]["diagnostics"]
        );
    })
}

// ════════════════════════════════════════════════════════════════════════════
// PR #79 round 3: the pull-based counterpart. A client advertising
// `workspace.configuration = true` is answered via `workspace/configuration`
// requests; the per-folder settings pulled that way must reach each folder
// context and refresh its cached values.
// ════════════════════════════════════════════════════════════════════════════

/// Spawn a session whose client advertises `workspace.configuration = true`
/// (the pull path). The server sends a `workspace/configuration` request
/// during `initialized`; the harness answers it with default (empty)
/// settings so the server unblocks and the background indexer runs.
async fn spawn_pull_session(root: &Path) -> (Session, tokio::task::JoinHandle<()>) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move {
        let _ = ry_lsp::run_with(server_reader, server_writer).await;
    });
    let mut session = LspSession::new(client_reader, client_writer);
    session
        .initialize_with_capabilities(root, json!({ "workspace": { "configuration": true } }))
        .await
        .unwrap();
    // Answer the initial `workspace/configuration` pull during `initialized`
    // with default settings (one per folder root, then a root-scoped item).
    session
        .respond_to_request("workspace/configuration", json!([{}, {}]))
        .await
        .unwrap();
    // Wait for background indexing to populate disk_files.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    (session, server)
}

#[test]
fn did_change_configuration_pull_applies_per_folder_settings() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // Same RY090/min-confidence setup as the test above.
        fixture
            .write_file("R/diag.R", "z <- length(xx = 1L)\n")
            .unwrap();

        let (mut session, _server) = spawn_pull_session(fixture.root()).await;

        let diag_uri = file_uri(&fixture.path("R/diag.R")).unwrap();

        // Open and capture the initial publication: RY090 must be present at
        // the default confidence (the initial pull returned empty settings).
        let mark0 = session.publication_mark();
        session
            .open(&diag_uri, 1, "z <- length(xx = 1L)\n")
            .await
            .unwrap();
        let initial = session
            .published_diagnostics_after(&diag_uri, mark0)
            .await
            .unwrap();
        assert!(
            count_code(&initial, "RY090") >= 1,
            "RY090 should be present at the default confidence; got: {:?}",
            initial["params"]["diagnostics"]
        );

        // Trigger the pull-based refresh: `workspace/didChangeConfiguration`
        // with no inline settings (the server pulls instead). Answer the
        // server's `workspace/configuration` request with settings raising
        // minConfidence — one per folder root, then a root-scoped item.
        let mark1 = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": {} }),
            )
            .await
            .unwrap();
        session
            .respond_to_request(
                "workspace/configuration",
                json!([
                    { "minConfidence": "high" },
                    { "minConfidence": "high" },
                ]),
            )
            .await
            .unwrap();
        let after = session
            .published_diagnostics_after(&diag_uri, mark1)
            .await
            .unwrap();

        // The pull must have reached the owning folder context and refreshed
        // its cached min_confidence, suppressing RY090 — without a file edit
        // or restart. The pre-fix pull stored only into the server-wide
        // `folder_settings`/`global_settings` and recomputed each context from
        // its unchanged `folder_settings`, so the cached value stayed stale.
        assert_eq!(
            count_code(&after, "RY090"),
            0,
            "RY090 should be suppressed after the pull raises minConfidence; got: {:?}",
            after["params"]["diagnostics"]
        );
    })
}
