//! R sources changed outside open buffers reach the disk index, and
//! closing a file analyzes what the disk holds. Every persisted transition
//! converges with a fresh server on the same final files.
//!
//! `a.R` and `z.R` both define `f`, `use.R` calls `f()`, and only the
//! character `f` makes RY040 fire in `use.R`, so the published diagnostics
//! show which file's bytes the server analyzed.

use crate::harness;

use harness::{
    assert_converges, cli_diagnostics, drain_publications, file_uri, has_ry040, join_session,
    notify_changed, notify_created, notify_deleted, spawn_session, spawn_watching_session,
    sync_barrier,
};
use ry_lsp::test_seam::Seam;
use ry_testkit::{FixtureProject, rpc_receive_timeout};
use serde_json::{Value, json};

/// `f <- function() "str"` — the character variant of `f`.
const A_CHAR: &str = "f <- function() \"str\"\n";
/// `f <- function() 1L` — the integer variant of `f`.
const F_INT: &str = "f <- function() 1L\n";
/// Calls the shadowed `f` in a shape that only mismatches for the
/// character variant.
const USE: &str = "x <- f() + 1L\n";

/// A fresh server over the same final filesystem, opening only `use.R`:
/// the oracle every persisted transition must converge with.
async fn fresh_use_diagnostics(fixture: &FixtureProject) -> Value {
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;
    let publish = harness::open_published(&mut session, &use_uri, 1, USE).await;
    join_session(session, server).await;
    publish
}

/// The server must actually watch R sources and reports: the registration
/// has to carry the source glob, not just handler behavior for faked events.
#[tokio::test]
async fn watcher_registration_includes_r_sources() {
    let fixture = FixtureProject::empty().unwrap();
    let (mut session, server) =
        spawn_session(&[fixture.root()], harness::watching_capabilities(), None).await;
    let globs: Vec<Value> = harness::answer_watcher_registration(&mut session)
        .await
        .into_iter()
        .map(|watcher| watcher["globPattern"].clone())
        .collect();
    assert!(
        globs
            .iter()
            .any(|glob| *glob == json!("**/*.{R,r,S,s,q,[Rr][Mm][Dd],[Qq][Mm][Dd]}")),
        "registration must watch R source files, got: {globs:?}"
    );
    join_session(session, server).await;
}

/// A watched R-source event that arrives while the initial index is in
/// flight must neither strand the session nor lose to the scan's older
/// read of the same file.
#[tokio::test]
async fn watched_event_during_initial_index_keeps_diagnostics_flowing() {
    // `a.R` alone defines `f` (character), so `use.R` carries RY040
    // once the session converges.
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    ry_lsp::test_seam::arm(Seam::Scan);
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    ry_lsp::test_seam::wait_arrived(Seam::Scan).await;

    // Rewrite the unopened `a.R` and deliver its watched change while
    // the initial scan, which read the character `f`, waits to install.
    std::fs::write(fixture.path("a.R"), F_INT).unwrap();
    notify_changed(&mut session, &a_uri).await;
    // The event applies after the scan's install, so its refreshed
    // bytes win over the scan's older read.
    let mark = session.publication_mark();
    session.open(&use_uri, 1, USE).await.unwrap();
    ry_lsp::test_seam::release(Seam::Scan);
    let publish = session
        .published_diagnostics_after(&use_uri, mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&publish),
        "the refreshed integer f must win over the initial scan's read: {publish}"
    );
    assert_converges(
        &publish,
        &fresh_use_diagnostics(&fixture).await,
        "watched event during initial index",
    );

    join_session(session, server).await;
}

/// Editing an unopened `.R` file on disk and forwarding the watched event
/// must refresh cross-file diagnostics without any open document changing.
#[tokio::test]
async fn unopened_edit_refreshes_cross_file_diagnostics() {
    // `z.R` on disk defines the integer `f`, which wins; `use.R` is
    // clean.
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", F_INT).unwrap();
    fixture.write_file("z.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let z_uri = file_uri(&fixture.path("z.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &use_uri).await;

    let first = harness::open_published(&mut session, &use_uri, 1, USE).await;
    assert!(
        !has_ry040(&first),
        "integer f must win before the on-disk edit"
    );

    // Rewrite `z.R` on disk to the character variant and deliver the
    // watched-file change the client would send for the save.
    std::fs::write(fixture.path("z.R"), A_CHAR).unwrap();
    let edit_mark = session.publication_mark();
    notify_changed(&mut session, &z_uri).await;
    let after = session
        .published_diagnostics_after(&use_uri, edit_mark)
        .await
        .unwrap();
    assert!(
        has_ry040(&after),
        "the on-disk edit to z.R must flip the winning f to its character variant"
    );
    assert_converges(
        &after,
        &fresh_use_diagnostics(&fixture).await,
        "unopened edit",
    );

    join_session(session, server).await;
}

/// Creating a new unopened `.R` file and forwarding the watched event
/// must make its definitions visible to cross-file analysis.
#[tokio::test]
async fn unopened_create_refreshes_cross_file_diagnostics() {
    // Only `a.R` (character `f`) and `use.R` exist: RY040 fires.
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &use_uri).await;

    let first = harness::open_published(&mut session, &use_uri, 1, USE).await;
    assert!(has_ry040(&first), "character f must win before z.R exists");

    // Create `z.R` on disk with the integer `f`, which sorts after
    // `a.R` and wins; forward the creation event.
    fixture.write_file("z.R", F_INT).unwrap();
    let z_uri = file_uri(&fixture.path("z.R"));
    let create_mark = session.publication_mark();
    notify_created(&mut session, &z_uri).await;
    let after = session
        .published_diagnostics_after(&use_uri, create_mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&after),
        "the created z.R's integer f must win once indexed"
    );
    assert_converges(
        &after,
        &fresh_use_diagnostics(&fixture).await,
        "unopened create",
    );

    join_session(session, server).await;
}

/// Deleting an unopened `.R` file and forwarding the watched event must
/// drop its definitions from cross-file analysis.
#[tokio::test]
async fn unopened_delete_refreshes_cross_file_diagnostics() {
    // `z.R` (integer `f`, wins) exists alongside `a.R` (character).
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("z.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let z_uri = file_uri(&fixture.path("z.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &use_uri).await;

    let first = harness::open_published(&mut session, &use_uri, 1, USE).await;
    assert!(!has_ry040(&first), "integer f must win before the deletion");

    std::fs::remove_file(fixture.path("z.R")).unwrap();
    let delete_mark = session.publication_mark();
    notify_deleted(&mut session, &z_uri).await;
    let after = session
        .published_diagnostics_after(&use_uri, delete_mark)
        .await
        .unwrap();
    assert!(
        has_ry040(&after),
        "deleting z.R must restore a.R's character f as the winner"
    );
    assert_converges(
        &after,
        &fresh_use_diagnostics(&fixture).await,
        "unopened delete",
    );

    join_session(session, server).await;
}

/// Save-then-close analyzes the saved bytes. `a.R` is the only `f` and
/// starts as the character variant. The buffer is edited to the integer
/// variant, saved with the client's save and watched events, and closed:
/// the closed `a.R` must mean the integer `f`, like a fresh server and
/// `ry check` on the final files.
#[tokio::test]
async fn save_then_close_converges_with_fresh_server() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &use_uri).await;

    let mark = session.publication_mark();
    session.open(&a_uri, 1, A_CHAR).await.unwrap();
    session.open(&use_uri, 1, USE).await.unwrap();
    let first = session
        .published_diagnostics_after(&use_uri, mark)
        .await
        .unwrap();
    assert!(has_ry040(&first), "character f must win while open");
    drain_publications(&mut session, &use_uri).await;

    let edit_mark = session.publication_mark();
    session
        .change(&a_uri, 2, json!([{ "text": F_INT }]))
        .await
        .unwrap();
    let edited = session
        .published_diagnostics_after(&use_uri, edit_mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&edited),
        "the edited integer f must clear RY040 while open"
    );

    // The watched refresh is a no-op while the buffer shadows the disk,
    // and it may publish nothing. Drain it so the close mark sees only the
    // close's publication.
    fixture.write_file("a.R", F_INT).unwrap();
    session
        .notify(
            "textDocument/didSave",
            json!({"textDocument": {"uri": a_uri}}),
        )
        .await
        .unwrap();
    notify_changed(&mut session, &a_uri).await;
    sync_barrier(&mut session, &use_uri).await;
    drain_publications(&mut session, &use_uri).await;

    let close_mark = session.publication_mark();
    session.close(&a_uri).await.unwrap();
    let after = session
        .published_diagnostics_after(&use_uri, close_mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&after),
        "save-then-close must keep the saved integer f for a.R"
    );
    assert_converges(
        &after,
        &fresh_use_diagnostics(&fixture).await,
        "save-then-close",
    );
    join_session(session, server).await;
    let cli = cli_diagnostics(fixture.root(), fixture.root(), &[]);
    assert!(cli.is_empty(), "CLI on the final tree is clean: {cli:?}");
}

/// Discard-then-close analyzes what the disk holds. `a.R` starts as the
/// integer variant. The buffer is edited to the character variant and
/// saved, then edited back to the integer variant without saving and
/// closed: the closed `a.R` must mean the saved character `f` again, like
/// a fresh server and `ry check` on the final files.
#[tokio::test]
async fn discard_then_close_converges_with_fresh_server() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &use_uri).await;

    let mark = session.publication_mark();
    session.open(&a_uri, 1, F_INT).await.unwrap();
    session.open(&use_uri, 1, USE).await.unwrap();
    let first = session
        .published_diagnostics_after(&use_uri, mark)
        .await
        .unwrap();
    assert!(!has_ry040(&first), "integer f must win while open");
    drain_publications(&mut session, &use_uri).await;

    let edit_mark = session.publication_mark();
    session
        .change(&a_uri, 2, json!([{ "text": A_CHAR }]))
        .await
        .unwrap();
    let edited = session
        .published_diagnostics_after(&use_uri, edit_mark)
        .await
        .unwrap();
    assert!(
        has_ry040(&edited),
        "the character edit must win while the buffer is open"
    );
    std::fs::write(fixture.path("a.R"), A_CHAR).unwrap();
    let discard_mark = session.publication_mark();
    session
        .change(&a_uri, 3, json!([{ "text": F_INT }]))
        .await
        .unwrap();
    let discarded = session
        .published_diagnostics_after(&use_uri, discard_mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&discarded),
        "the unsaved integer buffer must clean use.R: {discarded}"
    );

    let close_mark = session.publication_mark();
    session.close(&a_uri).await.unwrap();
    let after = session
        .published_diagnostics_after(&use_uri, close_mark)
        .await
        .unwrap();
    assert!(
        has_ry040(&after),
        "discard-then-close must restore the saved character f for a.R"
    );
    assert_converges(
        &after,
        &fresh_use_diagnostics(&fixture).await,
        "discard-then-close",
    );
    join_session(session, server).await;
    let cli = cli_diagnostics(fixture.root(), fixture.root(), &[]);
    assert!(
        cli.iter()
            .any(|p| p.path.ends_with("use.R") && p.code == "RY040"),
        "CLI on the final tree flags use.R: {cli:?}"
    );
}

/// Send a watched event for a nonexistent `DESCRIPTION` path: the
/// handler classifies it as a resolution-input change by URI suffix
/// alone and runs a full background scan to completion. This drives a
/// real scan without touching the fixture. The URI is built by
/// appending to the canonicalized root URI — no such file exists, so
/// the testkit's canonicalizing `file_uri` cannot encode it directly.
async fn trigger_full_scan(session: &mut harness::ClientSession, root: &std::path::Path) {
    let fake = format!("{}/DESCRIPTION", file_uri(root).as_str());
    notify_changed(session, &fake).await;
}

/// A watched edit queued behind an in-flight full scan applies after the
/// scan's install: the scan read the character `f` and parks before its
/// install; the disk moves to the integer `f` and its event arrives; once
/// released, the scan installs its older read and the refresh then lands
/// the newer bytes.
#[tokio::test]
async fn watched_edit_queued_behind_a_scan_lands_after_it() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    let first = harness::open_published(&mut session, &use_uri, 1, USE).await;
    assert!(
        has_ry040(&first),
        "character f must win before the scenario"
    );

    ry_lsp::test_seam::arm(Seam::Scan);
    trigger_full_scan(&mut session, fixture.root()).await;
    ry_lsp::test_seam::wait_arrived(Seam::Scan).await;

    // Strictly after the scan's walk: move the disk and deliver the
    // watched event, which queues behind the parked scan.
    std::fs::write(fixture.path("a.R"), F_INT).unwrap();
    notify_changed(&mut session, &a_uri).await;
    ry_lsp::test_seam::release(Seam::Scan);
    // A no-op edit publishes what the index holds after every writer the
    // test already waited for.
    let after = harness::change_published(&mut session, &use_uri, 2, USE).await;
    assert!(
        !has_ry040(&after),
        "the refresh queued behind the scan must win over its older read: {after}"
    );
    assert_converges(
        &after,
        &fresh_use_diagnostics(&fixture).await,
        "watched edit queued behind a scan",
    );

    join_session(session, server).await;
}

/// Save-then-close while a full scan holding the pre-save bytes is in
/// flight: the close-time re-read applies after the scan's install, so
/// the saved bytes survive.
#[tokio::test]
async fn close_queued_behind_a_scan_keeps_the_saved_bytes() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    let first = harness::open_published(&mut session, &use_uri, 1, USE).await;
    assert!(
        has_ry040(&first),
        "character f must win before the scenario"
    );

    // A full scan walks the still-character disk and parks before its
    // install, holding pre-save bytes.
    ry_lsp::test_seam::arm(Seam::Scan);
    trigger_full_scan(&mut session, fixture.root()).await;
    ry_lsp::test_seam::wait_arrived(Seam::Scan).await;

    // Edit `a.R` to the integer variant, save it, and close it while the
    // scan is parked.
    session.open(&a_uri, 1, A_CHAR).await.unwrap();
    session
        .change(&a_uri, 2, json!([{ "text": F_INT }]))
        .await
        .unwrap();
    std::fs::write(fixture.path("a.R"), F_INT).unwrap();
    session.close(&a_uri).await.unwrap();
    ry_lsp::test_seam::release(Seam::Scan);
    // A no-op edit publishes what the index holds after every writer the
    // test already waited for.
    let after = harness::change_published(&mut session, &use_uri, 2, USE).await;
    assert!(
        !has_ry040(&after),
        "save-then-close must keep the saved integer f even with a scan in flight: {after}"
    );
    assert_converges(
        &after,
        &fresh_use_diagnostics(&fixture).await,
        "close queued behind a scan",
    );

    join_session(session, server).await;
}

/// The initial scan starts only after the watcher registration was
/// answered: a disk edit between a scan's read and the client installing
/// its watchers would produce no event, leaving the read stale.
#[tokio::test]
async fn initial_scan_waits_for_watcher_registration() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    ry_lsp::test_seam::arm(Seam::Scan);
    let (mut session, server) =
        spawn_session(&[fixture.root()], harness::watching_capabilities(), None).await;
    let early = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        ry_lsp::test_seam::wait_arrived(Seam::Scan),
    )
    .await;
    assert!(
        early.is_err(),
        "the initial scan read the workspace before the watchers were registered"
    );
    harness::answer_watcher_registration(&mut session).await;
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_arrived(Seam::Scan),
    )
    .await
    .expect("the initial scan must run once the registration is answered");
    ry_lsp::test_seam::release(Seam::Scan);
    join_session(session, server).await;
}
