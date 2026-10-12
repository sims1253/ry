//! #528: project diagnostics for unopened files must be republished
//! when a watched-file event fixes them, even with zero documents open.
//!
//! The server publishes project diagnostics for closed files (the Problems
//! panel shows them while another file is open), but the publish pass only
//! ran when an open document scheduled it. With no documents open, a
//! watched-file event that fixed an indexed file refreshed the parse but
//! never republished: republishing zero open documents only cleared
//! paths that left the index — a still-indexed, still-eligible fixed file kept its pre-fix
//! squiggles until some unrelated document opened.
//!
//! The test below pins the fixed contract: open `main.R` (clean) so the
//! project pass publishes RY010 for the never-opened `util.R`, close
//! everything, fix `util.R` on disk, forward the watched event, and
//! require an (empty) publication for `util.R`. Without the fix the final
//! await times out and the test fails.

mod harness;

use harness::{
    file_uri, join_session, normalize_diagnostics, spawn_session, spawn_watching_session,
    sync_barrier,
};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

fn has_ry010(publish: &Value) -> bool {
    normalize_diagnostics(publish)
        .iter()
        .any(|diagnostic| diagnostic["code"] == json!("RY010"))
}

#[tokio::test]
async fn watched_fix_with_no_open_documents_republishes_the_closed_file() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", "w <- 1L\n").unwrap();
    fixture
        .write_file("util.R", "x <- never_bound_here\n")
        .unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    let util_uri = file_uri(&fixture.path("util.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &main_uri).await;

    // Opening clean `main.R` drives a project pass that also publishes
    // RY010 for the never-opened `util.R`: the stale squiggle the bug
    // would leave behind.
    let mark = session.publication_mark();
    session.open(&main_uri, 1, "w <- 1L\n").await.unwrap();
    let main_publish = session
        .published_diagnostics_after(&main_uri, mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&main_publish).is_empty(),
        "main.R must be clean: {main_publish}"
    );
    let stale = session
        .published_diagnostics_after(&util_uri, mark)
        .await
        .unwrap();
    assert!(
        has_ry010(&stale),
        "the project pass must publish RY010 for closed util.R: {stale}"
    );

    // Close everything: `util.R`'s squiggle legitimately stays (project
    // diagnostics for unopened files), tracked in `published_uris`.
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": main_uri}}),
        )
        .await
        .unwrap();
    sync_barrier(&mut session, &main_uri).await;

    // Fix `util.R` on disk and forward the watched event with zero
    // open documents. The refresh lands, and the server must
    // republish the fixed path through the debounce.
    std::fs::write(fixture.path("util.R"), "x <- 1L\n").unwrap();
    let fix_mark = session.publication_mark();
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": util_uri, "type": 2}]}),
        )
        .await
        .unwrap();
    // Without the fix no publication for `util.R` ever arrives and
    // this await times out, failing the test.
    let fixed = session
        .published_diagnostics_after(&util_uri, fix_mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&fixed).is_empty(),
        "the watched fix must clear util.R's RY010 with no documents open: {fixed}"
    );

    join_session(session, server).await;
}

/// Closing the last open document before its debounced pass ran ends with
/// the close's empty publication: the pending pass must not republish the
/// file's disk diagnostics afterwards.
#[tokio::test]
async fn closing_the_last_document_before_its_pass_publishes_nothing_more() {
    let fixture = FixtureProject::empty().unwrap();
    let source = "x <- never_bound_here\n";
    fixture.write_file("util.R", source).unwrap();
    let util_uri = file_uri(&fixture.path("util.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &util_uri).await;

    let mark = session.publication_mark();
    session.open(&util_uri, 1, source).await.unwrap();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": util_uri}}),
        )
        .await
        .unwrap();
    // A pass may legitimately publish while the buffer is open; the
    // close's clear is the last word.
    let mut mark = mark;
    loop {
        let publish = session
            .published_diagnostics_after(&util_uri, mark)
            .await
            .unwrap();
        if normalize_diagnostics(&publish).is_empty() {
            break;
        }
        mark = session.publication_mark();
    }
    let after = session.publication_mark();
    let leaked = tokio::time::timeout(
        std::time::Duration::from_millis(700),
        session.published_diagnostics_after(&util_uri, after),
    )
    .await;
    assert!(
        leaked.is_err(),
        "no publication may follow the close's clear; got {:?}",
        leaked.ok()
    );

    join_session(session, server).await;
}
