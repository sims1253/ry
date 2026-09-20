//! #490: deterministic cross-file definition shadowing in the LSP.
//!
//! `Project` resolves a top-level name defined in several files by merge
//! order (the later file wins). The LSP assembles that order as ONE
//! path-keyed source view: eligible disk entries with the disk twin of
//! every open path removed, each eligible open buffer inserted at its
//! own path, and the unified collection sorted once by path — the CLI's
//! sorted discovery order. An open buffer is authoritative for its own
//! path only: it replaces that path's disk bytes, but it does NOT
//! outrank a different closed file that sorts after it. The earlier
//! contract (all open documents layered after all disk entries) made
//! merely opening a byte-identical file flip the winning definition —
//! an outcome no edit and no CLI run could reproduce.
//!
//! `a.R` and `z.R` both define `f`; `use.R` calls `f()`. When `z.R`'s
//! integer `f` wins, `f() + 1L` is clean; when `a.R`'s character `f`
//! wins, `use.R` carries RY040 — the observable that flips.

mod harness;

use harness::{
    Published, file_uri, join_session, normalize_diagnostics, published_from_cli_value,
    spawn_session, sync_barrier,
};
use ry_testkit::{FixtureProject, rpc_receive_timeout};
use serde_json::{Value, json};

/// Idle window for `quiesce_diagnostics`; comfortably exceeds the
/// server's 180ms debounce so a multi-URI burst is fully collected.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(500);

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

/// `ry check --output-format json` on the fixture tree, normalized for
/// comparison with LSP publications. The CLI is the fresh-project oracle
/// every subset/state below must converge to. The subprocess runs on the
/// blocking pool: a child wait on the executor thread would stall the
/// in-process server task sharing the single-threaded runtime.
async fn cli_diagnostics(fixture: &FixtureProject) -> Vec<Published> {
    let root = fixture.root().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        // The binary resolves inside the closure too: the first
        // `ry_binary()` call runs a cargo build, which is just as
        // blocking as the child wait.
        std::process::Command::new(harness::ry_binary())
            .current_dir(&root)
            .args(["check", "--output-format", "json"])
            .arg(&root)
            .output()
    })
    .await
    .expect("CLI subprocess task")
    .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "CLI stdout was not JSON diagnostics ({error}); stdout head: {:?}, stderr head: {:?}",
            String::from_utf8_lossy(&output.stdout[..output.stdout.len().min(256)]),
            String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(256)]),
        )
    });
    values
        .into_iter()
        .map(|value| published_from_cli_value(&value, fixture.root()))
        .collect()
}

/// Cap on publications consumed by [`drain_publications`]. This
/// fixture publishes at most one diagnostic per file across three
/// files per pass; sixteen covers several passes with wide margin. A
/// server publishing more than this is stuck, and the drain reports
/// its last message instead of hanging.
const DRAIN_PUBLICATION_CAP: u32 = 16;

/// Consume every queued or in-flight `publishDiagnostics` for `uri`
/// until an idle window passes with none. After this returns, a fresh
/// publication mark can only be satisfied by a publication caused by
/// later actions. The idle window detects end-of-stream the same way
/// `quiesce_diagnostics` does; it does not wait for computation, and a
/// suppressed (deduplicated) no-op refresh simply ends the first
/// window. The drain is bounded so a server stuck publishing fails the
/// test with what it last sent instead of hanging.
async fn drain_publications(session: &mut harness::ClientSession, uri: &str) {
    let mut drained = 0u32;
    loop {
        let mark = session.publication_mark();
        match tokio::time::timeout(DRAIN, session.published_diagnostics_after(uri, mark)).await {
            Ok(result) => {
                let received = result.expect("publication receive error during drain");
                drained += 1;
                assert!(
                    drained <= DRAIN_PUBLICATION_CAP,
                    "still receiving publications after {drained} drained; last: {received}"
                );
            }
            Err(_) => return,
        }
    }
}

/// Capabilities advertising dynamic watched-file registration, so tests
/// that forward `workspace/didChangeWatchedFiles` events match a real
/// client (see `watched_closed_publish.rs`).
fn watching_capabilities() -> Value {
    json!({"workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}})
}

/// Answer the server's `client/registerCapability` request. The
/// sessions that call this advertise dynamic watched-file registration,
/// so the request must arrive and its response must succeed; a missing
/// handshake would only surface later as a missing watched-event
/// publication.
async fn answer_watcher_registration(session: &mut harness::ClientSession) {
    let result = tokio::time::timeout(
        rpc_receive_timeout(),
        session.respond_to_request("client/registerCapability", json!(null)),
    )
    .await;
    match result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => panic!("registerCapability response failed: {e}"),
        Err(_) => panic!(
            "server never sent client/registerCapability despite advertising watched-files registration"
        ),
    }
}

/// Close and reopen of an unchanged `a.R` must not flip which `f` wins.
#[test]
fn close_reopen_does_not_flip_the_winning_definition() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("z.R", F_INT).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let z_uri = file_uri(&fixture.path("z.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // All three open: canonical order is [a.R, use.R, z.R], so z.R wins.
        let mark = session.publication_mark();
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        session.open(&z_uri, 1, F_INT).await.unwrap();
        session.open(&use_uri, 1, USE).await.unwrap();
        let first = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&first),
            "z.R's integer f must win while all files are open"
        );

        // did_close: a.R returns via the disk index. Canonical order is
        // [a.R (disk), use.R, z.R] — z.R still wins.
        let close_mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
        let closed = session
            .published_diagnostics_after(&use_uri, close_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&closed),
            "closing a.R must not change the winner while z.R defines f"
        );

        // Reopen unchanged: without canonical ordering the re-added a.R
        // lands at the end of the merge order and its character f wins.
        let reopen_mark = session.publication_mark();
        session.open(&a_uri, 2, A_CHAR).await.unwrap();
        let reopened = session
            .published_diagnostics_after(&use_uri, reopen_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&reopened),
            "reopening unchanged a.R must not flip the winning f to its character variant"
        );

        join_session(session, server).await;
    })
}

/// Same-path buffer authority: an unsaved edit replaces the edited
/// path's OWN disk bytes. `a.R` is the only definer of `f` and holds
/// the character variant on disk (fresh CLI: RY040 in `use.R`); editing
/// the open buffer to the integer variant must clean `use.R` exactly
/// like a scratch CLI tree whose `a.R` carries the buffer bytes. This
/// is the control proving the buffer still owns its path — the
/// cross-path tests below must not weaken it.
#[test]
fn same_path_buffer_replaces_its_own_disk_bytes() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        // Neighboring valid control: a scratch tree with the integer
        // variant stays clean; the original tree must keep flagging so
        // the assertion cannot pass vacuously.
        let scratch = FixtureProject::empty().unwrap();
        scratch.write_file("a.R", F_INT).unwrap();
        scratch.write_file("use.R", USE).unwrap();
        let char_tree = cli_diagnostics(&fixture).await;
        assert!(
            char_tree
                .iter()
                .any(|p| p.path.ends_with("use.R") && p.code == "RY040"),
            "fresh CLI on the char-f tree must flag use.R: {char_tree:?}"
        );
        let scratch_cli = cli_diagnostics(&scratch).await;
        assert!(
            scratch_cli.is_empty(),
            "scratch CLI with the buffer bytes (int f) must be clean"
        );

        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // The disk bytes resolve while a.R is closed: char f wins.
        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        let disk_state = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            has_ry040(&disk_state),
            "the on-disk character f must flag use.R while a.R is closed"
        );

        // Unsaved edit to the integer variant: the buffer replaces
        // a.R's own disk bytes, so use.R must go clean. The open's own
        // publication is awaited first: a single mark across
        // open+change could be satisfied by the pre-change state when
        // the debounce lands between them.
        let open_mark = session.publication_mark();
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        let _ = session
            .published_diagnostics_after(&use_uri, open_mark)
            .await
            .unwrap();
        drain_publications(&mut session, &use_uri).await;
        let edit_mark = session.publication_mark();
        session
            .change(&a_uri, 2, json!([{"text": F_INT}]))
            .await
            .unwrap();
        let edited = session
            .published_diagnostics_after(&use_uri, edit_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&edited),
            "the unsaved integer buffer must replace a.R's own disk bytes: {edited}"
        );

        join_session(session, server).await;
    })
}

/// Path-local authority: opening (or editing) `a.R` grants no priority
/// over a DIFFERENT closed file that sorts after it. Both files define
/// the integer `f` on disk; the open buffer carries the character
/// variant. The unified path-sorted view keeps `z.R` last, so its
/// integer `f` keeps winning. Under the old open-layered-last contract
/// this exact scenario fabricated RY040 — the reproduced P2 finding.
#[test]
fn open_buffer_does_not_outrank_a_later_sorted_closed_file() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", F_INT).unwrap();
        fixture.write_file("z.R", F_INT).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // Opening a.R with the character variant in the buffer: the
        // buffer owns a.R's path, but z.R (closed, on disk) still sorts
        // last and wins.
        // One publication per state transition: a mark after opening
        // use.R, awaited, then a separate mark before opening a.R, so
        // the assertion observes the post-open-a.R publication instead
        // of whichever debounced intermediate lands first.
        let open_use_mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        let _ = session
            .published_diagnostics_after(&use_uri, open_use_mark)
            .await
            .unwrap();
        drain_publications(&mut session, &use_uri).await;

        let open_a_mark = session.publication_mark();
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        let opened = session
            .published_diagnostics_after(&use_uri, open_a_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&opened),
            "opening a.R alone must not flip the winner to its buffer: {opened}"
        );

        join_session(session, server).await;
    })
}

/// Open/closed subset parity: for every non-empty subset of
/// {a.R, z.R, use.R} (same bytes as disk), the eventual `use.R`
/// diagnostics equal the fresh-project result — asserted in full
/// against the CLI oracle hoisted above the loop, which proves the
/// whole tree clean and so requires use.R to publish no diagnostics at
/// all, not merely no RY040. Publication completion is required per
/// state (an awaited publication, never an absent one). The empty
/// subset is covered separately below: nothing schedules a publish
/// pass there, so its final analysis is forced with a watched-file
/// event, which republishes closed files (#528) and is therefore proof
/// the pass ran.
#[test]
fn open_closed_subsets_agree_with_the_fresh_project() {
    run(async {
        // The fresh-project oracle the subsets must converge to, run
        // explicitly instead of hardcoded: the integer `f` sorts last,
        // so the whole tree checks clean.
        {
            let oracle_fixture = FixtureProject::empty().unwrap();
            oracle_fixture.write_file("a.R", A_CHAR).unwrap();
            oracle_fixture.write_file("z.R", F_INT).unwrap();
            oracle_fixture.write_file("use.R", USE).unwrap();
            let oracle = cli_diagnostics(&oracle_fixture).await;
            assert!(
                oracle.is_empty(),
                "fresh CLI must be clean with the integer f last: {oracle:?}"
            );
        }
        let subsets: Vec<Vec<bool>> = vec![
            vec![true, false, false],
            vec![false, true, false],
            vec![false, false, true],
            vec![true, true, false],
            vec![true, false, true],
            vec![false, true, true],
            vec![true, true, true],
        ];
        for subset in &subsets {
            let fixture = FixtureProject::empty().unwrap();
            fixture.write_file("a.R", A_CHAR).unwrap();
            fixture.write_file("z.R", F_INT).unwrap();
            fixture.write_file("use.R", USE).unwrap();
            let a_uri = file_uri(&fixture.path("a.R"));
            let z_uri = file_uri(&fixture.path("z.R"));
            let use_uri = file_uri(&fixture.path("use.R"));
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
            sync_barrier(&mut session, &use_uri).await;

            let mark = session.publication_mark();
            if subset[0] {
                session.open(&a_uri, 1, A_CHAR).await.unwrap();
            }
            if subset[1] {
                session.open(&z_uri, 1, F_INT).await.unwrap();
            }
            if subset[2] {
                session.open(&use_uri, 1, USE).await.unwrap();
            }
            let store = session
                .quiesce_diagnostics(&use_uri, mark, DRAIN)
                .await
                .unwrap();
            let use_diags = store.get(&use_uri).expect("use.R must be analyzed");
            assert!(
                use_diags.is_empty(),
                "subset {subset:?}: use.R must match the clean CLI oracle exactly, got: {use_diags:?}"
            );

            join_session(session, server).await;
        }

        // All-closed state: a fresh watching session never opens a
        // document; the watched event is the publication trigger.
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("z.R", F_INT).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        answer_watcher_registration(&mut session).await;
        sync_barrier(&mut session, &use_uri).await;
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": use_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        let store = session
            .quiesce_diagnostics(&use_uri, mark, DRAIN)
            .await
            .unwrap();
        let use_diags = store.get(&use_uri).expect("use.R must be analyzed");
        assert!(
            use_diags.is_empty(),
            "all-closed state must match the clean CLI oracle exactly, got: {use_diags:?}"
        );
        join_session(session, server).await;
    })
}

/// Open/close of an unchanged `a.R` (and `z.R`) must not change
/// `use.R`'s semantics step by step — the direct LSP replay of the
/// reproduced three-state finding, now asserted instead of printed.
#[test]
fn opening_and_closing_unchanged_documents_never_flips_use_r() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("z.R", F_INT).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let z_uri = file_uri(&fixture.path("z.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // State 1: only use.R open. Disk index holds a.R (char) and
        // z.R (int); sorted order [a.R, use.R, z.R] keeps z.R's f last.
        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        let state1 = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(!has_ry040(&state1), "state1 must be clean: {state1}");

        // State 2: open a.R byte-identical (z.R stays closed). This is
        // the reproduced flip: open status used to outrank z.R's disk
        // definition.
        let mark = session.publication_mark();
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        let state2 = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&state2),
            "opening byte-identical a.R must not flip the winner: {state2}"
        );

        // State 3: open z.R as well.
        let mark = session.publication_mark();
        session.open(&z_uri, 1, F_INT).await.unwrap();
        let state3 = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(!has_ry040(&state3), "state3 must be clean: {state3}");

        // State 4: close a.R again (z.R stays open).
        let mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
        let state4 = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(!has_ry040(&state4), "closing a.R must stay clean: {state4}");

        join_session(session, server).await;
    })
}

/// Discard-close: closing the edited buffer WITHOUT saving returns the
/// path to its on-disk bytes, so the result matches the final disk
/// tree (char `f` only definer on disk -> RY040 returns in `use.R`).
#[test]
fn discard_close_returns_to_the_disk_bytes() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // Open both, awaiting each open's publication before editing:
        // a single mark across opens+change could observe the
        // intermediate pre-change state under a split debounce.
        let open_use_mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        let _ = session
            .published_diagnostics_after(&use_uri, open_use_mark)
            .await
            .unwrap();
        drain_publications(&mut session, &use_uri).await;
        let open_a_mark = session.publication_mark();
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        let _ = session
            .published_diagnostics_after(&use_uri, open_a_mark)
            .await
            .unwrap();
        drain_publications(&mut session, &use_uri).await;
        let edit_mark = session.publication_mark();
        session
            .change(&a_uri, 2, json!([{"text": F_INT}]))
            .await
            .unwrap();
        let edited = session
            .published_diagnostics_after(&use_uri, edit_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&edited),
            "the unsaved integer buffer must clean use.R: {edited}"
        );

        // Discard-close: disk still holds the character variant, so the
        // close-time re-read must bring RY040 back, matching the tree.
        let close_mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
        let closed = session
            .published_diagnostics_after(&use_uri, close_mark)
            .await
            .unwrap();
        assert!(
            has_ry040(&closed),
            "discard-close must restore the on-disk semantics: {closed}"
        );
        let cli = cli_diagnostics(&fixture).await;
        assert!(
            cli.iter()
                .any(|p| p.path.ends_with("use.R") && p.code == "RY040"),
            "CLI on the final tree flags use.R: {cli:?}"
        );

        join_session(session, server).await;
    })
}

/// Save-close: writing the buffer bytes to disk and forwarding the
/// save/watched events must leave the post-close analysis matching the
/// final on-disk tree (integer `f` everywhere -> clean).
#[test]
fn save_close_matches_the_final_disk_tree() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", A_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        answer_watcher_registration(&mut session).await;
        sync_barrier(&mut session, &use_uri).await;

        // One publication mark per transition: a single mark spanning
        // the opens and the change could be satisfied by an
        // intermediate pre-change state when the debounce splits them.
        let open_use_mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        let _ = session
            .published_diagnostics_after(&use_uri, open_use_mark)
            .await
            .unwrap();
        drain_publications(&mut session, &use_uri).await;
        let open_a_mark = session.publication_mark();
        session.open(&a_uri, 1, A_CHAR).await.unwrap();
        let _ = session
            .published_diagnostics_after(&use_uri, open_a_mark)
            .await
            .unwrap();
        drain_publications(&mut session, &use_uri).await;
        let edit_mark = session.publication_mark();
        session
            .change(&a_uri, 2, json!([{"text": F_INT}]))
            .await
            .unwrap();
        let edited = session
            .published_diagnostics_after(&use_uri, edit_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&edited),
            "the unsaved integer buffer must clean use.R: {edited}"
        );

        // Save: the client writes the buffer to disk and reports the
        // save plus the watched change. The watched refresh is a no-op
        // while a.R is still open (its buffer shadows the disk bytes),
        // so no publication is guaranteed between the save and the
        // close below — the close is where the saved bytes matter.
        fixture.write_file("a.R", F_INT).unwrap();
        session
            .notify(
                "textDocument/didSave",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": a_uri, "type": 2}]}),
            )
            .await
            .unwrap();

        // Drain the no-op watched refresh so close_mark cannot be
        // satisfied by a queued pre-close publication: the barrier's
        // inlayHint response does not flush publishes already queued,
        // and the deduplicated refresh may publish nothing at all.
        sync_barrier(&mut session, &use_uri).await;
        drain_publications(&mut session, &use_uri).await;
        // Close: the re-read finds the saved integer bytes; use.R stays
        // clean, matching the final disk tree.
        let close_mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
        let closed = session
            .published_diagnostics_after(&use_uri, close_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&closed),
            "save-close must keep the saved semantics: {closed}"
        );
        let final_cli = cli_diagnostics(&fixture).await;
        assert!(final_cli.is_empty(), "CLI on the final tree is clean");

        join_session(session, server).await;
    })
}

/// Cold-session determinism: two fresh sessions that open the same
/// documents in different orders must agree on which `f` wins.
#[test]
fn open_order_does_not_change_the_selected_definition() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(open_order_independence());
}

/// Open the defining documents (in the given order), then `use.R`, and
/// return the normalized diagnostics published for `use.R`.
async fn observe_use_diagnostics(
    fixture: &FixtureProject,
    defining: &[(&str, &str)],
    use_uri: &str,
) -> Vec<Value> {
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, use_uri).await;
    let mark = session.publication_mark();
    for (uri, text) in defining {
        session.open(uri, 1, text).await.unwrap();
    }
    session.open(use_uri, 1, USE).await.unwrap();
    let publish = session
        .published_diagnostics_after(use_uri, mark)
        .await
        .unwrap();
    join_session(session, server).await;
    normalize_diagnostics(&publish)
}

async fn open_order_independence() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("z.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let z_uri = file_uri(&fixture.path("z.R"));
    let use_uri = file_uri(&fixture.path("use.R"));

    let forward =
        observe_use_diagnostics(&fixture, &[(&a_uri, A_CHAR), (&z_uri, F_INT)], &use_uri).await;
    let reverse =
        observe_use_diagnostics(&fixture, &[(&z_uri, F_INT), (&a_uri, A_CHAR)], &use_uri).await;
    assert_eq!(
        forward, reverse,
        "didOpen order must not influence which definition wins"
    );
    assert!(
        !forward.iter().any(|d| d["code"] == json!("RY040")),
        "z.R's integer f must win in canonical order: {forward:?}"
    );
}
