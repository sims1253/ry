//! Configuration-refresh and enable behavior for the LSP.
//!
//! `did_change_configuration_refreshes_cached_filters` verifies that a
//! `workspace/didChangeConfiguration` raising `ry.minConfidence` changes the
//! next published diagnostics without any file edit or restart. The cached
//! per-folder severity filter, min-confidence, and excludes are recomputed
//! outside the publish loop on every configuration change; without that
//! refresh a stale cached value would persist until a filesystem rebuild or
//! server restart.
//!
//! `enable_false_skips_diagnostics_for_the_folder` verifies that a folder
//! whose settings set `enable: false` is skipped: opening a file there
//! publishes an empty diagnostics set instead of check results.
//! `enable_false_skips_inlay_hints_for_the_folder` verifies the on-demand
//! half: `textDocument/inlayHint` returns null there instead of hints.

use ry_testkit::{FixtureProject, file_uri};
use serde_json::{Value, json};

mod harness;

use harness::spawn_session_with_options;

/// Run a future on a current-thread tokio runtime (same pattern as the
/// session tests).
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

        let (mut session, _server) =
            spawn_session_with_options(fixture.root(), json!({}), Value::Null).await;

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

#[test]
fn did_change_configuration_pull_applies_per_folder_settings() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // Same RY090/min-confidence setup as the test above.
        fixture
            .write_file("R/diag.R", "z <- length(xx = 1L)\n")
            .unwrap();

        let (mut session, _server) = spawn_session_with_options(
            fixture.root(),
            json!({ "workspace": { "configuration": true } }),
            Value::Null,
        )
        .await;
        // Answer the initial `workspace/configuration` pull during
        // `initialized` with default settings (one per folder root, then a
        // root-scoped item).
        session
            .respond_to_request("workspace/configuration", json!([{}, {}]))
            .await
            .unwrap();

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

// ════════════════════════════════════════════════════════════════════════════
// `enable: false` skips analysis and publishing for the folder
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn enable_false_skips_diagnostics_for_the_folder() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // RY090 would fire at the default confidence when enabled.
        fixture
            .write_file("R/diag.R", "z <- length(xx = 1L)\n")
            .unwrap();

        let (mut session, server) = spawn_session_with_options(
            fixture.root(),
            json!({}),
            json!({
                "settings": [{"enable": false}],
                "globalSettings": {}
            }),
        )
        .await;

        let diag_uri = file_uri(&fixture.path("R/diag.R")).unwrap();
        let mark = session.publication_mark();
        session
            .open(&diag_uri, 1, "z <- length(xx = 1L)\n")
            .await
            .unwrap();
        let publish = session
            .published_diagnostics_after(&diag_uri, mark)
            .await
            .unwrap();

        let count = publish["params"]["diagnostics"]
            .as_array()
            .map(|diags| diags.len())
            .unwrap_or(0);
        assert_eq!(
            count, 0,
            "enable: false must publish an empty diagnostics set; got: {publish}"
        );
        let _ = session.shutdown().await;
        drop(session);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
    })
}

#[test]
fn enable_false_skips_inlay_hints_for_the_folder() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // `x <- 1L` yields one integer hint when the folder is enabled
        // (pinned by `inlay_hint_does_not_cross_workspace_roots`).
        fixture.write_file("R/hint.R", "x <- 1L\n").unwrap();

        let (mut session, server) = spawn_session_with_options(
            fixture.root(),
            json!({}),
            json!({
                "settings": [{"enable": false}],
                "globalSettings": {}
            }),
        )
        .await;

        let hint_uri = file_uri(&fixture.path("R/hint.R")).unwrap();
        session.open(&hint_uri, 1, "x <- 1L\n").await.unwrap();

        let result = session
            .request(
                "textDocument/inlayHint",
                json!({
                    "textDocument": {"uri": hint_uri},
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 1, "character": 0}
                    }
                }),
            )
            .await
            .unwrap();

        assert_eq!(
            result,
            serde_json::Value::Null,
            "enable: false must return null instead of inlay hints; got: {result}"
        );
        let _ = session.shutdown().await;
        drop(session);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
    })
}
