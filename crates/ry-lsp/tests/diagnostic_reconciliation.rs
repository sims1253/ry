//! #489: every URI that stops being analyzed must receive an empty
//! publication — all of them, not just the last-scheduled one.
//!
//! Two gaps combined into the bug:
//!
//! * `republish_all_open_documents` scheduled one debounced task per
//!   open document, but the debounce used a single workspace-wide
//!   generation, so of N scheduled URIs only the last task survived
//!   and the rest aborted as stale. Which URI survived followed
//!   `docs` HashMap iteration order.
//! * `publish_diagnostics` sent an empty publication only for its own
//!   requested URI when that URI was ineligible. Every other document
//!   that left the eligible set (folder disabled, file excluded) was
//!   filtered out of the check and never published again, and closed
//!   disk files a rescan dropped from the index had the same gap —
//!   nothing reconciled against the URIs that had received
//!   diagnostics before.
//!
//! The single-document cases were already covered and passed by the
//! surviving task's own URI, so every test here keeps at least two
//! documents in play and asserts the client's whole final diagnostic
//! store (the per-URI map `quiesce_diagnostics` returns, where the
//! last publication per URI wins), not the latest publication for one
//! document.

mod harness;

use harness::{file_uri, join_session, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Idle window for `quiesce_diagnostics`; comfortably exceeds the
/// server's 180ms debounce so a multi-URI burst is fully collected.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(500);

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

/// RY010 (unbound name) occurrences in one URI's stored publication.
fn ry010_count(diagnostics: &[Value]) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["code"] == json!("RY010"))
        .count()
}

/// The URI must be present in the final store with an empty set: the
/// bug left affected URIs absent entirely (no clearing publication) or
/// carrying their previous diagnostics.
fn assert_cleared(store: &BTreeMap<String, Vec<Value>>, uri: &str) {
    assert!(
        store.get(uri).is_some_and(Vec::is_empty),
        "{uri} must end with an empty publication; final store: {store:?}"
    );
}

fn assert_ry010(store: &BTreeMap<String, Vec<Value>>, uri: &str) {
    assert!(
        store
            .get(uri)
            .is_some_and(|diagnostics| ry010_count(diagnostics) == 1),
        "{uri} must end with exactly one RY010; final store: {store:?}"
    );
}

/// Disabling the folder with two documents open must clear both URIs.
/// On main only the URI carried by the surviving debounce task was
/// cleared; the other kept its squiggles indefinitely. Re-enabling
/// must restore both, proving the clear was a publication and not a
/// permanent silencing.
#[test]
fn disabling_the_folder_clears_every_open_document() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("a.R", "x <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("b.R", "y <- never_bound_here\n")
            .unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let b_uri = file_uri(&fixture.path("b.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;

        sync_barrier(&mut session, &a_uri).await;
        let mark = session.publication_mark();
        session
            .open(&a_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        session
            .open(&b_uri, 1, "y <- never_bound_here\n")
            .await
            .unwrap();
        let initial = session
            .quiesce_diagnostics(&a_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_ry010(&initial, &a_uri);
        assert_ry010(&initial, &b_uri);

        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": { "ry": { "enable": false } } }),
            )
            .await
            .unwrap();
        let disabled = session
            .quiesce_diagnostics(&a_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_cleared(&disabled, &a_uri);
        assert_cleared(&disabled, &b_uri);

        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": { "ry": { "enable": true } } }),
            )
            .await
            .unwrap();
        let enabled = session
            .quiesce_diagnostics(&a_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_ry010(&enabled, &a_uri);
        assert_ry010(&enabled, &b_uri);

        join_session(session, server).await;
    })
}

/// The mixed case: excluding one of two open documents. The excluded
/// URI must be cleared even though the still-eligible one also
/// republishes — when the surviving debounce task carried the eligible
/// URI, the excluded one was never rescheduled at all.
#[test]
fn excluding_one_of_two_open_documents_clears_only_the_excluded_one() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("a.R", "x <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("b.R", "y <- never_bound_here\n")
            .unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let b_uri = file_uri(&fixture.path("b.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;

        sync_barrier(&mut session, &a_uri).await;
        let mark = session.publication_mark();
        session
            .open(&a_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        session
            .open(&b_uri, 1, "y <- never_bound_here\n")
            .await
            .unwrap();
        let initial = session
            .quiesce_diagnostics(&a_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_ry010(&initial, &a_uri);
        assert_ry010(&initial, &b_uri);

        fixture
            .write_file("ry.toml", "exclude = [\"a.R\"]\n")
            .unwrap();
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": {} }),
            )
            .await
            .unwrap();
        let excluded = session
            .quiesce_diagnostics(&b_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_cleared(&excluded, &a_uri);
        assert_ry010(&excluded, &b_uri);

        join_session(session, server).await;
    })
}

/// Closed disk files a rescan drops — deleted from disk, or newly
/// excluded — must get an empty publication too. `keep.R` stays open
/// and drives the publish; `a.R` and `c.R` are never opened, so their
/// diagnostics arrive through the background index while `keep.R` is
/// open.
#[test]
fn closed_disk_files_dropped_by_a_rescan_get_an_empty_publication() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("ry.toml", "\n").unwrap();
        fixture
            .write_file("a.R", "x <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("c.R", "z <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("keep.R", "w <- never_bound_here\n")
            .unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let c_uri = file_uri(&fixture.path("c.R"));
        let keep_uri = file_uri(&fixture.path("keep.R"));
        let config_uri = file_uri(&fixture.path("ry.toml"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;

        sync_barrier(&mut session, &keep_uri).await;
        let mark = session.publication_mark();
        session
            .open(&keep_uri, 1, "w <- never_bound_here\n")
            .await
            .unwrap();
        let initial = session
            .quiesce_diagnostics(&keep_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_ry010(&initial, &a_uri);
        assert_ry010(&initial, &c_uri);
        assert_ry010(&initial, &keep_uri);

        // Deletion: the rescan drops a.R from the index. A plain .R
        // deletion is not itself a watched-file trigger, so the
        // notification rides the ry.toml change the state machine's
        // DeleteFile operation uses.
        std::fs::remove_file(fixture.path("a.R")).unwrap();
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({ "changes": [{ "uri": config_uri, "type": 2 }] }),
            )
            .await
            .unwrap();
        let deleted = session
            .quiesce_diagnostics(&keep_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_cleared(&deleted, &a_uri);
        assert_ry010(&deleted, &keep_uri);

        // New exclusion: c.R stays on disk and indexed, but the
        // config change drops it from the eligible set.
        fixture
            .write_file("ry.toml", "exclude = [\"c.R\"]\n")
            .unwrap();
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": {} }),
            )
            .await
            .unwrap();
        let excluded = session
            .quiesce_diagnostics(&keep_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_cleared(&excluded, &c_uri);
        assert_ry010(&excluded, &keep_uri);

        join_session(session, server).await;
    })
}
