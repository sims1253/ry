//! #528: project diagnostics for unopened files must be republished
//! when a watched-file event fixes them, even with zero documents open.
//!
//! The server publishes project diagnostics for closed files (the Problems
//! panel shows them while another file is open), but the publish pass only
//! ran when an open document scheduled it. With no documents open, a
//! watched-file event that fixed an indexed file refreshed the parse but
//! never republished: `republish_all_open_documents` degenerates to
//! `clear_dropped_diagnostics`, which only clears paths that left the
//! index — a still-indexed, still-eligible fixed file kept its pre-fix
//! squiggles until some unrelated document opened.
//!
//! The test below pins the fixed contract: open `main.R` (clean) so the
//! project pass publishes RY010 for the never-opened `util.R`, close
//! everything, fix `util.R` on disk, forward the watched event, and
//! require an (empty) publication for `util.R`. Without the fix the final
//! await times out and the test fails.

mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session, sync_barrier};
use ry_testkit::{FixtureProject, rpc_receive_timeout};
use serde_json::{Value, json};

/// Capabilities advertising dynamic watched-file registration, so the
/// server registers its R source globs and the test client answers the
/// registration request.
fn watching_capabilities() -> Value {
    json!({"workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}})
}

/// Answer the server's `client/registerCapability` request (if any) so the
/// session proceeds; the globs themselves are pinned elsewhere. The wait
/// uses the shared receive budget (#551): the request is server work that
/// parallel test load can starve past a fixed 5s.
async fn answer_watcher_registration(session: &mut harness::ClientSession) {
    let _: Option<Result<Value, _>> = tokio::time::timeout(
        rpc_receive_timeout(),
        session.respond_to_request("client/registerCapability", json!(null)),
    )
    .await
    .ok()
    .map(|result| result.map_err(|_| ()));
}

fn has_ry010(publish: &Value) -> bool {
    normalize_diagnostics(publish)
        .iter()
        .any(|diagnostic| diagnostic["code"] == json!("RY010"))
}

#[test]
fn watched_fix_with_no_open_documents_republishes_the_closed_file() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("main.R", "w <- 1L\n").unwrap();
        fixture
            .write_file("util.R", "x <- never_bound_here\n")
            .unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let util_uri = file_uri(&fixture.path("util.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        answer_watcher_registration(&mut session).await;
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
        // diagnostics for unopened files), tracked in `published_paths`.
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
    });
}

/// Shared prologue of the two #551 interleaving tests: the three-file
/// fixture (`main.R` clean, `util.R` unbound, `other.R` a bystander), a
/// watching session, the settle pass that proves the initial index
/// committed (opening `main.R` publishes both `main.R` and the stale
/// RY010 for the never-opened `util.R`), and a gated `didClose` whose
/// close-time re-read is drained through the commit gate so no refresh
/// is in flight when the scenario arms — the re-read parks whenever it
/// arrives (close handlers dispatch concurrently with later client
/// traffic, the very window the bug needs).
async fn settled_closed_fixture() -> (
    FixtureProject,
    harness::ClientSession,
    tokio::task::JoinHandle<()>,
    String,
    String,
) {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", "w <- 1L\n").unwrap();
    fixture
        .write_file("util.R", "x <- never_bound_here\n")
        .unwrap();
    fixture.write_file("other.R", "o <- 1L\n").unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    let util_uri = file_uri(&fixture.path("util.R"));
    let other_uri = file_uri(&fixture.path("other.R"));
    let (mut session, server) =
        spawn_session(&[fixture.root()], watching_capabilities(), None).await;
    answer_watcher_registration(&mut session).await;
    sync_barrier(&mut session, &main_uri).await;

    let mark = session.publication_mark();
    session.open(&main_uri, 1, "w <- 1L\n").await.unwrap();
    session
        .published_diagnostics_after(&main_uri, mark)
        .await
        .unwrap();
    session
        .published_diagnostics_after(&util_uri, mark)
        .await
        .unwrap();
    ry_lsp::test_seam::arm_refresh_commit();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": main_uri}}),
        )
        .await
        .unwrap();
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_refresh_commit(),
    )
    .await
    .expect("the close-time re-read must reach the armed commit gate");
    ry_lsp::test_seam::release_refresh_commit();
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_refresh_landed(),
    )
    .await
    .expect("the parked close-time refresh must make its commit decision");
    sync_barrier(&mut session, &main_uri).await;
    (fixture, session, server, util_uri, other_uri)
}

/// #551: a watched refresh that loses the index-generation race to an
/// unrelated concurrent refresh must not lose the publication. The
/// per-file refresh carries only its own path, but its commit was
/// discarded outright whenever the generation moved during its blocking
/// read — and the generation moves for ANY landed writer, including a
/// close-time re-read or another path's watched event whose landing
/// says nothing about this path's bytes. With no open document nothing
/// else republishes the path, so the watched fix stranded: the test
/// client's convergence await burned its whole budget and failed
/// (bimodally — 0.4s green or a full-budget timeout, nothing between).
///
/// The test pins the discriminating interleaving deterministically
/// through the existing commit gates: park the watched refresh for
/// `util.R` at its commit (post-read), land an unrelated refresh for
/// `other.R` while it is parked (proving the landing through the
/// post-commit gate), then release. Without the retry the parked commit
/// sees the moved generation and discards — the fix never lands and the
/// await below times out (or, if another publish pass re-reaches the
/// path first, republishes the STALE squiggle). With the retry the
/// refresh re-reads current disk, lands the fixed bytes, and the empty
/// publication arrives.
#[test]
fn watched_fix_losing_the_generation_race_still_republishes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (fixture, mut session, server, util_uri, other_uri) =
            settled_closed_fixture().await;

        // Fix `util.R` on disk; its watched refresh parks at its commit.
        std::fs::write(fixture.path("util.R"), "x <- 1L\n").unwrap();
        ry_lsp::test_seam::arm_refresh_commit();
        let fix_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": util_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_refresh_commit(),
        )
        .await
        .expect("the watched refresh must reach the armed commit gate");

        // While `util.R`'s refresh is parked, an unrelated refresh for
        // `other.R` lands — the one-shot arm was consumed by the parked
        // arrival — bumping the generation. The post-commit gate proves
        // the landing happened before the release below.
        std::fs::write(fixture.path("other.R"), "o <- 2L\n").unwrap();
        ry_lsp::test_seam::arm_post_refresh_commit();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": other_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_post_refresh_commit(),
        )
        .await
        .expect("the unrelated refresh must land and reach the post-commit gate");

        // Release the parked commit: its snapshot generation is stale.
        ry_lsp::test_seam::release_refresh_commit();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_refresh_landed(),
        )
        .await
        .expect("the released refresh must make its (retrying) commit decision");
        ry_lsp::test_seam::release_post_refresh_commit();

        // The retry must land the fixed bytes and republish them.
        let fixed = session
            .published_diagnostics_after(&util_uri, fix_mark)
            .await
            .unwrap();
        assert!(
            normalize_diagnostics(&fixed).is_empty(),
            "a watched fix whose refresh loses the generation race must still land and republish: {fixed}"
        );

        join_session(session, server).await;
    });
}

/// #551's second rung: a watched refresh that loses the generation race
/// TWICE converges through the full-scan backstop, and the backstop must
/// publish too — the double-race case may differ from the single-race one
/// only in HOW the bytes land (scan versus retry), never in whether they
/// reach the client. The escalation threads the scan's verdict: a landed
/// scan reports `true`, the watched handler adds the path to its landed
/// list, and `schedule_closed_file_publish` drives the republish (the
/// #528 convention the other no-open-document scan call sites follow).
///
/// Deterministic through the gates, one loss per attempt: attempt 0 parks
/// at the armed commit gate, an unrelated refresh lands (generation
/// moves), the gate is re-armed while attempt 0 is still parked so the
/// retry parks too, a second unrelated refresh lands, and the release
/// exhausts the ladder — the scan then rendezvoused through its own
/// commit gate before landing.
#[test]
fn watched_fix_exhausting_the_retry_ladder_publishes_through_the_scan() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (fixture, mut session, server, util_uri, other_uri) =
            settled_closed_fixture().await;

        // Fix `util.R`; its watched refresh parks at its commit (attempt 0).
        std::fs::write(fixture.path("util.R"), "x <- 1L\n").unwrap();
        ry_lsp::test_seam::arm_refresh_commit();
        let fix_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": util_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_refresh_commit(),
        )
        .await
        .expect("attempt 0 must reach the armed commit gate");

        // First loss: an unrelated refresh for `other.R` lands while
        // attempt 0 is parked, moving the generation.
        std::fs::write(fixture.path("other.R"), "o <- 2L\n").unwrap();
        ry_lsp::test_seam::arm_post_refresh_commit();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": other_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_post_refresh_commit(),
        )
        .await
        .expect("the first unrelated refresh must land and reach the post-commit gate");

        // Re-arm while attempt 0 is still parked so the retry parks too,
        // then release: attempt 0's commit fails the moved generation and
        // the retry re-reads and parks at the re-armed gate. The pair is
        // order-safe by construction — the release wakes the PARKED
        // attempt 0 (its waiter exists), while the re-arm only sets the
        // one-shot flag the NEXT arrival (the retry) consumes — so do not
        // "fix" this by waiting between them: the next wait below is the
        // retry's arrival, not attempt 0's exit.
        ry_lsp::test_seam::arm_refresh_commit();
        ry_lsp::test_seam::release_refresh_commit();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_refresh_commit(),
        )
        .await
        .expect("the retry must reach the re-armed commit gate");

        // Second loss: another unrelated landing moves the generation
        // again, so the retry's commit will fail and the ladder exhausts.
        std::fs::write(fixture.path("other.R"), "o <- 3L\n").unwrap();
        ry_lsp::test_seam::arm_post_refresh_commit();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": other_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_post_refresh_commit(),
        )
        .await
        .expect("the second unrelated refresh must land and reach the post-commit gate");

        // Arm the scan's commit gate, then release the retry: its commit
        // fails the generation check, the escalation spawns the backstop
        // scan, and the scan parks at its own gate — the rendezvous that
        // proves the backstop fired.
        ry_lsp::test_seam::arm_scan_commit();
        ry_lsp::test_seam::release_refresh_commit();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_scan_commit(),
        )
        .await
        .expect("the escalation must spawn the backstop scan");
        ry_lsp::test_seam::release_scan_commit();

        // The landed scan reports true, so the watched handler publishes.
        let fixed = session
            .published_diagnostics_after(&util_uri, fix_mark)
            .await
            .unwrap();
        assert!(
            normalize_diagnostics(&fixed).is_empty(),
            "a watched fix exhausting the retry ladder must publish through the backstop scan: {fixed}"
        );

        // Release the two unrelated refreshes parked at the post-commit
        // gate — one notify per parked waiter — so teardown leaves no
        // paused writers behind and the gate state matches the
        // single-race test's.
        ry_lsp::test_seam::release_post_refresh_commit();
        ry_lsp::test_seam::release_post_refresh_commit();

        join_session(session, server).await;
    });
}
