//! #486: the disk index goes stale when R sources change outside open
//! buffers, and closing a saved file restores the last indexed snapshot.
//!
//! The language server used to register no watched globs for R sources and
//! to early-return on their events, and `did_close` never re-read the
//! file — so after "edit → save → close" (with another document open) the
//! closed file was re-analyzed from its initialize-time snapshot until some
//! unrelated event (config/DESCRIPTION/NAMESPACE/native/data change)
//! triggered a full reindex.
//!
//! These tests pin the converged contract against a fresh server over the
//! same final filesystem: every persisted source transition (unopened-file
//! edit/create/delete, save-then-close) agrees with a cold start, while the
//! open buffer always overrides disk.
//!
//! The observable is cross-file definition shadowing (see
//! `cross_file_shadowing.rs`): `a.R` and `z.R` both define `f`, `use.R`
//! calls `f()`, and only the character variant of `f` produces RY040 in
//! `use.R` — so which file's content the server analyzes is directly
//! visible in the published diagnostics.

mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

/// `f <- function() "str"` — the character variant of `f`.
const A_CHAR: &str = "f <- function() \"str\"\n";
/// `f <- function() 1L` — the integer variant of `f`.
const F_INT: &str = "f <- function() 1L\n";
/// Calls the shadowed `f` in a shape that only mismatches for the
/// character variant.
const USE: &str = "x <- f() + 1L\n";

fn has_ry040(publish: &Value) -> bool {
    normalize_diagnostics(publish)
        .iter()
        .any(|diagnostic| diagnostic["code"] == json!("RY040"))
}

/// Capabilities advertising dynamic watched-file registration, so the
/// server registers its R source globs and the test client answers the
/// registration request.
fn watching_capabilities() -> Value {
    json!({"workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}})
}

/// Answer the server's `client/registerCapability` request (if any) and
/// return the requested watcher globs. Sessions spawned without
/// registration support produce no request; the helper then yields an
/// empty list instead of blocking.
async fn take_watcher_globs(session: &mut harness::ClientSession) -> Vec<Value> {
    let pending: Option<Value> = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        session.respond_to_request("client/registerCapability", json!(null)),
    )
    .await
    .ok()
    .and_then(|result| result.ok());
    pending
        .as_ref()
        .and_then(|request| {
            request["params"]["registrations"]
                .as_array()?
                .iter()
                .flat_map(|registration| {
                    registration["registerOptions"]["watchers"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                })
                .map(|watcher| watcher["globPattern"].clone())
                .collect::<Vec<_>>()
                .into()
        })
        .unwrap_or_default()
}

/// Open only `use.R` (whose content never changes) and return the fresh
/// publication for it after `mark`.
async fn open_use_and_observe(
    session: &mut harness::ClientSession,
    use_uri: &str,
    mark: u64,
) -> Value {
    session.open(use_uri, 1, USE).await.unwrap();
    session
        .published_diagnostics_after(use_uri, mark)
        .await
        .unwrap()
}

/// A fresh server over the same final filesystem, opening only `use.R`:
/// the oracle every persisted transition must converge with.
async fn fresh_use_diagnostics(fixture: &FixtureProject) -> Value {
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;
    let mark = session.publication_mark();
    let publish = open_use_and_observe(&mut session, &use_uri, mark).await;
    join_session(session, server).await;
    publish
}

fn assert_converges(live: &Value, fresh: &Value, context: &str) {
    assert_eq!(
        normalize_diagnostics(live),
        normalize_diagnostics(fresh),
        "{context}: live server must match a fresh server on the final filesystem"
    );
}

/// The server must actually watch R sources: the registration has to
/// carry an R source glob, not just handler behavior for faked events.
#[test]
fn watcher_registration_includes_r_sources() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        let globs = take_watcher_globs(&mut session).await;
        assert!(
            globs.iter().any(|glob| *glob == json!("**/*.{R,r,S,s,q}")),
            "registration must watch R source files, got: {globs:?}"
        );
        join_session(session, server).await;
    });
}

/// A watched R-source event that lands while the initial background
/// index is still in flight must not strand the session: the per-file
/// path retires the in-flight pass via the index generation, so when the
/// retired pass is the initial one a fresh pass takes over clearing
/// `initial_index_pending` — otherwise publications would stay gated for
/// the rest of the session.
#[test]
fn watched_event_during_initial_index_keeps_diagnostics_flowing() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // `a.R` alone defines `f` (character), so `use.R` carries RY040
        // once the session converges.
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        ry_lsp::test_seam::arm_initial_index();
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        ry_lsp::test_seam::wait_initial_index().await;

        // Rewrite the unopened `a.R` and deliver its watched change while
        // the initial pass is paused: this retires the pass.
        std::fs::write(fixture.path("a.R"), F_INT).unwrap();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        // Opening `use.R` must still produce diagnostics: the retired
        // initial pass is replaced, not left stranding the flag.
        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        ry_lsp::test_seam::release_initial_index();
        let publish = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&publish),
            "the refreshed integer f must win after the initial index is retired: {publish}"
        );
        assert_converges(
            &publish,
            &fresh_use_diagnostics(&fixture).await,
            "watched event during initial index",
        );

        join_session(session, server).await;
    });
}

/// Editing an unopened `.R` file on disk and forwarding the watched event
/// must refresh cross-file diagnostics without any open document changing.
#[test]
fn unopened_edit_refreshes_cross_file_diagnostics() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // `z.R` on disk defines the integer `f`, which wins; `use.R` is
        // clean.
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", F_INT).unwrap();
        fixture.write_file("z.R", F_INT).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let z_uri = file_uri(&fixture.path("z.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        let first = open_use_and_observe(&mut session, &use_uri, mark).await;
        assert!(
            !has_ry040(&first),
            "integer f must win before the on-disk edit"
        );

        // Rewrite `z.R` on disk to the character variant and deliver the
        // watched-file change the client would send for the save.
        std::fs::write(fixture.path("z.R"), A_CHAR).unwrap();
        let edit_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": z_uri, "type": 2}]}),
            )
            .await
            .unwrap();
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
    });
}

/// Creating a new unopened `.R` file and forwarding the watched event
/// must make its definitions visible to cross-file analysis.
#[test]
fn unopened_create_refreshes_cross_file_diagnostics() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // Only `a.R` (character `f`) and `use.R` exist: RY040 fires.
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        let first = open_use_and_observe(&mut session, &use_uri, mark).await;
        assert!(has_ry040(&first), "character f must win before z.R exists");

        // Create `z.R` on disk with the integer `f`, which sorts after
        // `a.R` and wins; forward the creation event.
        fixture.write_file("z.R", F_INT).unwrap();
        let z_uri = file_uri(&fixture.path("z.R"));
        let create_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": z_uri, "type": 1}]}),
            )
            .await
            .unwrap();
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
    });
}

/// Deleting an unopened `.R` file and forwarding the watched event must
/// drop its definitions from cross-file analysis.
#[test]
fn unopened_delete_refreshes_cross_file_diagnostics() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // `z.R` (integer `f`, wins) exists alongside `a.R` (character).
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("z.R", F_INT).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let z_uri = file_uri(&fixture.path("z.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        let first = open_use_and_observe(&mut session, &use_uri, mark).await;
        assert!(!has_ry040(&first), "integer f must win before the deletion");

        std::fs::remove_file(fixture.path("z.R")).unwrap();
        let delete_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": z_uri, "type": 3}]}),
            )
            .await
            .unwrap();
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
    });
}

/// Save-then-close must analyze the saved bytes: with `a.R` the only
/// definition of `f`, open it as the character variant, save the integer
/// variant to disk, close, and the closed file's indexed snapshot must
/// carry the integer `f` — matching a fresh server — rather than the
/// initialize-time character snapshot.
#[test]
fn save_then_close_converges_with_fresh_server() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // On disk `a.R` starts as the character variant; it is the only
        // `f`, so its content alone decides RY040 in `use.R`.
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        sync_barrier(&mut session, &use_uri).await;

        // Open both with the on-disk contents: character `f` wins, RY040.
        let mark = session.publication_mark();
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        session.open(&use_uri, 1, USE).await.unwrap();
        let first = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(has_ry040(&first), "character f must win while open");

        // Edit the open buffer to the integer variant (RY040 clears),
        // save it to disk (the client writes the file), then close. The
        // closed `a.R` must keep meaning integer `f`.
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
        std::fs::write(fixture.path("a.R"), F_INT).unwrap();
        let close_mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
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
    });
}

/// Discard-then-close must analyze whatever the disk holds: editing the
/// open buffer, saving, then editing again WITHOUT saving and closing
/// restores the saved bytes — the same bytes a fresh server reads.
#[test]
fn discard_then_close_converges_with_fresh_server() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // On disk `a.R` starts as the integer variant; it is the only
        // `f`, so its content alone decides RY040 in `use.R`.
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", F_INT).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        sync_barrier(&mut session, &use_uri).await;

        // Open both with the on-disk contents: integer `f` wins, clean.
        let mark = session.publication_mark();
        session.open(&a_uri, 1, F_INT).await.unwrap();
        session.open(&use_uri, 1, USE).await.unwrap();
        let first = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(!has_ry040(&first), "integer f must win while open");

        // Edit the buffer to the character variant (RY040 appears) and
        // save that to disk; then edit back to integer WITHOUT saving
        // (RY040 clears) and close. The disk holds the character `f`, so
        // the closed file must mean character `f` again.
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
        session
            .published_diagnostics_after(&use_uri, discard_mark)
            .await
            .unwrap();
        let close_mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
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
    });
}

/// Send a watched event for a nonexistent `DESCRIPTION` path: the
/// handler classifies it as a resolution-input change by URI suffix
/// alone and runs a full background scan to completion. This drives a
/// real scan without touching the fixture. The URI is built by
/// appending to the canonicalized root URI — no such file exists, so
/// the testkit's canonicalizing `file_uri` cannot encode it directly.
async fn trigger_full_scan(session: &mut harness::ClientSession, root: &std::path::Path) {
    let fake = format!("{}/DESCRIPTION", file_uri(root).as_str());
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": fake, "type": 2}]}),
        )
        .await
        .unwrap();
}

/// Open `use.R` and await its first publication. The publish path
/// returns early while `initial_index_pending` holds, and the flag
/// clears only at the initial scan's commit — so this publication
/// proves the initial index committed, and every generation the
/// scenario later captures starts from a settled session.
async fn open_use_settled(session: &mut harness::ClientSession, use_uri: &str) -> Value {
    session.open(use_uri, 1, USE).await.unwrap();
    session
        .published_diagnostics_after(use_uri, session.publication_mark())
        .await
        .unwrap()
}

/// A no-op edit (same text, bumped version) drives one debounced
/// publish of whatever the index currently holds, without changing
/// any file. The observation point every freshness test ends at: the
/// publication reflects the committed state at its snapshot, which is
/// strictly after every writer the test already rendezvoused with.
async fn observe_current_index(
    session: &mut harness::ClientSession,
    use_uri: &str,
    version: i32,
) -> Value {
    let mark = session.publication_mark();
    session
        .change(use_uri, version, json!([{ "text": USE }]))
        .await
        .unwrap();
    session
        .published_diagnostics_after(use_uri, mark)
        .await
        .unwrap()
}

/// #526, writer A vs writer B: a per-file refresh whose blocking read
/// saw older bytes must not install its parse over a newer full scan's
/// commit. The refresh reads the character `f`, pauses at its commit
/// gate; the disk moves to the integer `f`; a full scan walks and
/// commits the integer bytes under a newer generation; only then is
/// the refresh released. Without the generation check the stale parse
/// wins and RY040 appears; with it the scan's bytes survive.
#[test]
fn stale_refresh_loses_to_newer_scan() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        let first = open_use_settled(&mut session, &use_uri).await;
        assert!(
            has_ry040(&first),
            "character f must win before the scenario"
        );

        // The watched change for the still-character `a.R` starts a
        // refresh that reads the old bytes, then pauses at its commit.
        ry_lsp::test_seam::arm_refresh_commit();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        ry_lsp::test_seam::wait_refresh_commit().await;

        // The arrival signal is sent after the read completed, so this
        // write is strictly later: the refresh holds the old bytes.
        std::fs::write(fixture.path("a.R"), F_INT).unwrap();
        trigger_full_scan(&mut session, fixture.root()).await;
        sync_barrier(&mut session, &use_uri).await;
        // The scan ran to completion while the refresh waited: release
        // the stale commit and observe the surviving bytes.
        ry_lsp::test_seam::release_refresh_commit();
        ry_lsp::test_seam::wait_refresh_landed().await;
        let after = observe_current_index(&mut session, &use_uri, 2).await;
        assert!(
            !has_ry040(&after),
            "the scan's integer f must survive the stale refresh commit: {after}"
        );
        assert_converges(
            &after,
            &fresh_use_diagnostics(&fixture).await,
            "stale refresh vs newer scan",
        );

        join_session(session, server).await;
    });
}

/// #526, refresh vs refresh: two watched events for one path start two
/// refreshes, and the later event's bytes must win even when the
/// earlier refresh's commit lands last. The first refresh reads the
/// character `f` and pauses; the disk moves to the integer `f` and a
/// second refresh lands it (the one-shot arm is consumed, so the
/// second passes through); only then is the first released.
#[test]
fn overlapping_refreshes_last_event_wins() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        let first = open_use_settled(&mut session, &use_uri).await;
        assert!(
            has_ry040(&first),
            "character f must win before the scenario"
        );

        ry_lsp::test_seam::arm_refresh_commit();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        ry_lsp::test_seam::wait_refresh_commit().await;

        // Strictly after the first refresh's read: move the disk and
        // deliver the second event, whose refresh lands the new bytes
        // and retires the generation the first refresh captured.
        std::fs::write(fixture.path("a.R"), F_INT).unwrap();
        let second_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        let second = session
            .published_diagnostics_after(&use_uri, second_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&second),
            "the second refresh must land the integer f: {second}"
        );

        ry_lsp::test_seam::release_refresh_commit();
        ry_lsp::test_seam::wait_refresh_landed().await;
        let after = observe_current_index(&mut session, &use_uri, 2).await;
        assert!(
            !has_ry040(&after),
            "the late first refresh must not overwrite the second's integer f: {after}"
        );
        assert_converges(
            &after,
            &fresh_use_diagnostics(&fixture).await,
            "overlapping refreshes",
        );

        join_session(session, server).await;
    });
}

/// #538, same-path refreshes under one generation: two watched events
/// for one path start two refreshes that snapshot the SAME index
/// generation, and the generation protocol cannot order them — the
/// older read committing first installs its bytes and atomically bumps
/// the generation, so the newer read's commit then fails the
/// generation check and is discarded, leaving the stale contents
/// indexed until the next event for the path. The per-path refresh
/// epoch, claimed when each refresh STARTS (before its blocking read),
/// breaks the tie: a refresh a newer same-path refresh superseded can
/// never commit, whichever commit lines up on the state lock first.
/// This test pins the discriminating ordering the generation check
/// could not survive: the OLDER read commits FIRST and must lose.
/// Both refreshes park at the one-shot commit gate (the arm is
/// consumed per arrival, so arming again parks the second arrival
/// too); releases wake in arrival order — tokio's `notify_one` wakes
/// the oldest waiter (FIFO in the pinned implementation, not a
/// documented API guarantee) — so the older refresh, which arrived
/// first, makes its commit decision strictly before the newer one.
/// Should a future tokio ever wake out of arrival order, the newer
/// read would commit first and land in both worlds: the test would
/// pass vacuously, never flake.
#[test]
fn older_read_committing_first_loses_to_newer_refresh() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        let first = open_use_settled(&mut session, &use_uri).await;
        assert!(
            has_ry040(&first),
            "character f must win before the scenario"
        );

        // Refresh A reads the still-character `a.R` and parks at its
        // commit gate; the arrival signal is sent after the read, so
        // A's bytes are proven older than everything below.
        ry_lsp::test_seam::arm_refresh_commit();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        ry_lsp::test_seam::wait_refresh_commit().await;

        // Refresh B starts strictly after A's read — the re-armed gate
        // parks it too — and reads the integer `f`. B's start claims a
        // newer per-path epoch while A is still parked, but B's commit
        // decision is held at the gate: A commits first.
        std::fs::write(fixture.path("a.R"), F_INT).unwrap();
        ry_lsp::test_seam::arm_refresh_commit();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        ry_lsp::test_seam::wait_refresh_commit().await;

        // Release in arrival order: A commits first and must be
        // discarded WITHOUT bumping the generation, so B — epoch and
        // generation both still current — lands the integer bytes.
        ry_lsp::test_seam::release_refresh_commit();
        ry_lsp::test_seam::wait_refresh_landed().await;
        ry_lsp::test_seam::release_refresh_commit();
        ry_lsp::test_seam::wait_refresh_landed().await;
        let after = observe_current_index(&mut session, &use_uri, 2).await;
        assert!(
            !has_ry040(&after),
            "the older read committing first must not defeat the newer refresh: {after}"
        );
        assert_converges(
            &after,
            &fresh_use_diagnostics(&fixture).await,
            "older-read-first same-path refreshes",
        );

        join_session(session, server).await;
    });
}

/// #526, the close-time hole: `did_close` re-reads the closed file,
/// but a background scan in flight at close time — whose walk read
/// the file before the save — must not replace the whole map with its
/// older snapshot afterwards. The scan walks the character `f` and
/// pauses at its commit; only then are the integer bytes saved and
/// the file closed, so the close-time refresh lands newer bytes and
/// retires the scan. Without the close-time generation bump the
/// scan's pre-save snapshot wins and RY040 reappears.
#[test]
fn close_time_refresh_retires_in_flight_scan() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        let first = open_use_settled(&mut session, &use_uri).await;
        assert!(
            has_ry040(&first),
            "character f must win before the scenario"
        );

        // A full scan walks the still-character disk and pauses at its
        // commit, holding pre-save bytes.
        ry_lsp::test_seam::arm_scan_commit();
        trigger_full_scan(&mut session, fixture.root()).await;
        ry_lsp::test_seam::wait_scan_commit().await;

        // Strictly after the scan's walk: edit `a.R` to the integer
        // variant, save it, and close. The close-time refresh lands the
        // saved bytes; its commit gate rendezvous proves it.
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        session
            .change(&a_uri, 2, json!([{ "text": F_INT }]))
            .await
            .unwrap();
        std::fs::write(fixture.path("a.R"), F_INT).unwrap();
        ry_lsp::test_seam::arm_refresh_commit();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
        ry_lsp::test_seam::wait_refresh_commit().await;
        ry_lsp::test_seam::release_refresh_commit();
        ry_lsp::test_seam::wait_refresh_landed().await;

        // Consume did_close's own follow-up publication (integer-clean
        // in both revisions: the scan has not committed yet), then
        // release the scan and observe what survives it.
        let close_mark = session.publication_mark();
        sync_barrier(&mut session, &use_uri).await;
        let _ = session
            .published_diagnostics_after(&use_uri, close_mark)
            .await;
        let scan_mark = session.publication_mark();
        ry_lsp::test_seam::release_scan_commit();
        let after = session
            .published_diagnostics_after(&use_uri, scan_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&after),
            "save-then-close must keep the saved integer f even with a scan in flight: {after}"
        );
        assert_converges(
            &after,
            &fresh_use_diagnostics(&fixture).await,
            "close-time refresh vs in-flight scan",
        );

        join_session(session, server).await;
    });
}

/// #526, atomic retirement: a landed refresh must claim the next
/// generation in the same critical section as its map write — check,
/// insert, and bump under one lock hold — so a scan committing
/// between the insert and a delayed caller-side bump cannot win.
/// The scan walks the character `f` and pauses at its commit; only
/// then does the disk move to the integer `f` and the watched event
/// fire. The refresh's post-commit gate pauses it after its critical
/// section releases but before the caller acts: the arrival signal
/// proves the refresh already retired the scan. Released afterwards,
/// the scan must lose; had the fix not bumped atomically, the scan
/// would still be current there and its whole-map commit would
/// overwrite the refresh's newer bytes.
#[test]
fn landed_refresh_bumps_generation_atomically_with_insert() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        take_watcher_globs(&mut session).await;
        let first = open_use_settled(&mut session, &use_uri).await;
        assert!(
            has_ry040(&first),
            "character f must win before the scenario"
        );

        // The scan walks the still-character disk and pauses at its
        // commit, holding pre-save bytes under the current generation.
        ry_lsp::test_seam::arm_scan_commit();
        trigger_full_scan(&mut session, fixture.root()).await;
        ry_lsp::test_seam::wait_scan_commit().await;

        // Strictly after the scan's walk: move the disk to the integer
        // `f` and deliver the watched event. The post-commit gate
        // pauses the refresh after its insert-and-bump critical
        // section, before the caller-side retirement could run.
        std::fs::write(fixture.path("a.R"), F_INT).unwrap();
        ry_lsp::test_seam::arm_post_refresh_commit();
        let edit_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        ry_lsp::test_seam::wait_post_refresh_commit().await;
        // The refresh's atomic bump already retired the scan, so the
        // scan's release must land on a moved generation and lose.
        ry_lsp::test_seam::release_scan_commit();
        ry_lsp::test_seam::release_post_refresh_commit();
        let after = session
            .published_diagnostics_after(&use_uri, edit_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&after),
            "the atomic refresh must retire the in-flight scan before the caller runs: {after}"
        );
        assert_converges(
            &after,
            &fresh_use_diagnostics(&fixture).await,
            "atomic refresh retirement",
        );

        join_session(session, server).await;
    });
}
