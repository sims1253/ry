//! P1 (#551 successor): watched-file work must be RETAINED until the
//! final analysis converges — no rescue event allowed.
//!
//! `refresh_disk_entry`'s ladder retries one lost generation race and
//! escalates a second loss to a full backstop scan. The backstop itself
//! can lose: another landing during the scan's walk supersedes its
//! commit, the scan returns without installing, and the refresh used to
//! return `false` while forgetting the path entirely. With no open
//! document nothing republishes the path, and with no further event
//! nothing re-reads it — the watched fix or creation stranded forever.
//! The fix retains a per-path obligation (enqueued at the refresh's
//! epoch claim, before the losing read) and a reconciliation driver
//! that re-runs the landed-refresh pipeline — bytes, owning package
//! context, publication — until the obligation settles.
//!
//! The ladder interleave is driven deterministically through the
//! existing commit gates: A's attempt 0 parks at its commit, an
//! unrelated B lands (loss 1), the retry parks, C lands (loss 2), the
//! escalation scan parks at its commit, D lands (the scan's loss).
//! Then silence. A's final diagnostics must match A's current bytes.
//!
//! `a.R` carries a direct, independently checkable error
//! (`x <- never_bound_here`, RY010) — never a competing-definition
//! observable — with `main.R`/`b.R`/`c.R`/`d.R` as unrelated
//! bystanders.
//!
//! Two later hardening regressions live here too. The versioned
//! completion token: an older refresh's final acknowledgement must not
//! retire a newer event's re-armed obligation even after the newer
//! entry advanced to the context/publication phase — pinned with a
//! resolution-sensitive observable (a `load()` span inside an isolated
//! `DESCRIPTION` package) so only the surviving obligation's context
//! refresh can move the verdict. And the paced (not dropped) stall
//! bound: obligations outlive any finite supersession streak, with each
//! successive unsuccessful round provably waiting out its backoff rung
//! before the next dispatch begins, and no background pass per paced
//! retry.

mod harness;

use harness::{
    ClientSession, Published, file_uri, join_session, normalize_diagnostics,
    published_from_cli_value, spawn_session, sync_barrier,
};
use ry_testkit::{CliProcess, FixtureProject, rpc_receive_timeout};
use serde_json::{Value, json};

/// Await the next publication for `uri` whose diagnostics satisfy
/// `matches`, stepping the mark publication-by-publication so
/// intermediate (stale) publications from in-flight passes are skipped
/// instead of being mistaken for the final state — a fixed idle window
/// cannot do this, because under parallel test load the reconciliation
/// chain (driver round plus the 180ms debounce) can outlast any
/// comfortable window. Bounded by `tries` awaited publications; the
/// last await failing is the shared receive-budget failure bound.
async fn await_diagnostics_where(
    session: &mut ClientSession,
    uri: &str,
    matches: impl Fn(&[Value]) -> bool,
    tries: usize,
) -> Vec<Value> {
    let mut mark = session.publication_mark();
    for attempt in 0..tries {
        let publish = session
            .published_diagnostics_after(uri, mark)
            .await
            .unwrap_or_else(|error| {
                panic!("publication await failed on attempt {attempt}: {error}")
            });
        let diagnostics = normalize_diagnostics(&publish);
        if matches(&diagnostics) {
            return diagnostics;
        }
        mark = session.publication_mark();
    }
    panic!("no publication for {uri} matched within {tries} attempts");
}

/// Capabilities advertising dynamic watched-file registration, so the
/// tests forward `workspace/didChangeWatchedFiles` events like a real
/// client (see `watched_closed_publish.rs`).
fn watching_capabilities() -> Value {
    json!({"workspace": {"didChange_watchedFiles": {"dynamicRegistration": true}}})
}

/// Answer the server's `client/registerCapability` request (if any) so
/// the session proceeds; the globs themselves are pinned elsewhere.
async fn answer_watcher_registration(session: &mut ClientSession) {
    let _: Option<Result<Value, _>> = tokio::time::timeout(
        rpc_receive_timeout(),
        session.respond_to_request("client/registerCapability", json!(null)),
    )
    .await
    .ok()
    .map(|result| result.map_err(|_| ()));
}

fn has_ry010(diagnostics: &[Value]) -> bool {
    diagnostics.iter().any(|d| d["code"] == json!("RY010"))
}

/// Whether the diagnostics carry the RY010 report for the binding `` `a` ``
/// starting at `line` — the resolution-sensitive observable: whether a
/// use AFTER a `load()` call resolves depends on the installed
/// package-group context's load inventory (keyed by the load call's
/// byte offset in the file's parse), never on the re-parse alone.
fn has_ry010_for_a_at_line(diagnostics: &[Value], line: u64) -> bool {
    diagnostics.iter().any(|d| {
        d["code"] == json!("RY010")
            && d["message"]
                .as_str()
                .is_some_and(|message| message.contains("`a`"))
            && d["range"]["start"]["line"].as_u64() == Some(line)
    })
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
/// comparison with LSP publications: the fresh-analysis oracle the
/// convergence assertions are measured against.
fn cli_diagnostics(fixture: &FixtureProject) -> Vec<Published> {
    let output = CliProcess::new(harness::ry_binary())
        .check(fixture, fixture.root(), ["--output-format", "json"])
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    values
        .into_iter()
        .map(|value| published_from_cli_value(&value, fixture.root()))
        .collect()
}

fn cli_flags(fixture: &FixtureProject, name: &str) -> bool {
    cli_diagnostics(fixture)
        .iter()
        .any(|p| p.path.ends_with(name) && p.code == "RY010")
}

/// The settled five-file session the ladder tests build on: `main.R`
/// clean, `a.R` carrying `a_initial`, `b.R`/`c.R`/`d.R` bystanders, a
/// watching session whose initial index provably committed (opening
/// `main.R` publishes it AND `a.R`'s current state), and a gated
/// `didClose` of `main.R` whose close-time re-read is fully drained —
/// no refresh in flight when the scenario arms.
struct Ladder {
    fixture: FixtureProject,
    session: ClientSession,
    server: tokio::task::JoinHandle<()>,
    a_uri: String,
    b_uri: String,
    c_uri: String,
    d_uri: String,
}

async fn settled_ladder(a_initial: &str) -> Ladder {
    settled_ladder_in("a.R", a_initial, &[]).await
}

/// [`settled_ladder`] with the ladder's target path and extra non-source
/// files placed before the initial index: the acknowledgement-ordering
/// regression below points the target at a file inside its own
/// `DESCRIPTION` package and ships the package's metadata and `data.rda`
/// through `extra`, so the package-group isolation the test's observable
/// depends on exists from the first scan.
async fn settled_ladder_in(a_relative: &str, a_initial: &str, extra: &[(&str, &[u8])]) -> Ladder {
    let fixture = FixtureProject::empty().unwrap();
    for (relative, bytes) in extra {
        fixture.write_file(relative, *bytes).unwrap();
    }
    fixture.write_file("main.R", "w <- 1L\n").unwrap();
    fixture.write_file(a_relative, a_initial).unwrap();
    fixture.write_file("b.R", "o <- 1L\n").unwrap();
    fixture.write_file("c.R", "p <- 1L\n").unwrap();
    fixture.write_file("d.R", "q <- 1L\n").unwrap();
    let a_uri = file_uri(&fixture.path(a_relative));
    let b_uri = file_uri(&fixture.path("b.R"));
    let c_uri = file_uri(&fixture.path("c.R"));
    let d_uri = file_uri(&fixture.path("d.R"));
    let main_uri = file_uri(&fixture.path("main.R"));
    let (mut session, server) =
        spawn_session(&[fixture.root()], watching_capabilities(), None).await;
    answer_watcher_registration(&mut session).await;
    sync_barrier(&mut session, &main_uri).await;

    // Opening clean `main.R` drives a project pass that also publishes
    // `a.R`'s current state — the baseline the ladder then loses.
    let mark = session.publication_mark();
    session.open(&main_uri, 1, "w <- 1L\n").await.unwrap();
    session
        .published_diagnostics_after(&main_uri, mark)
        .await
        .unwrap();
    session
        .published_diagnostics_after(&a_uri, mark)
        .await
        .unwrap();

    // Close `main.R` with the close-time re-read drained through the
    // commit gates (the `settled_closed_fixture` convention), so the
    // ladder below starts from a quiescent index.
    ry_lsp::test_seam::arm_refresh_commit();
    let close_mark = session.publication_mark();
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
    // The close handler's tail — its context refresh, the obligation
    // completion, and the empty clear — must be provably finished
    // before the ladder proper arms: the empty publication is sent
    // AFTER all of them, so awaiting it here (instead of trusting the
    // commit-gate signal, which fires just before the commit critical
    // section) removes the ambient in-flight work that could race the
    // first ladder event under CI load.
    let cleared = session
        .published_diagnostics_after(&main_uri, close_mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&cleared).is_empty(),
        "the closed document's clear must be the close handler's last word"
    );
    sync_barrier(&mut session, &main_uri).await;

    Ladder {
        fixture,
        session,
        server,
        a_uri,
        b_uri,
        c_uri,
        d_uri,
    }
}

/// Write `name` and forward its watched event, proving the landing
/// through the post-commit gate (the `#551` convention). The paired
/// release keeps teardown free of parked writers.
async fn land_unrelated(ladder: &mut Ladder, name: &str, uri: &str, content: &str) {
    ladder.fixture.write_file(name, content).unwrap();
    ry_lsp::test_seam::arm_post_refresh_commit();
    ladder
        .session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": uri, "type": 2}]}),
        )
        .await
        .unwrap();
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_post_refresh_commit(),
    )
    .await
    .expect("the unrelated refresh must land and reach the post-commit gate");
    ry_lsp::test_seam::release_post_refresh_commit();
}

/// Land `name` and LEAVE its refresh parked at the post-commit gate: the
/// bytes are committed (the generation bump that defeats a parked
/// racer is already sealed) but the handler tail — its own group's
/// context refresh, its acknowledgement, and above all its epilogue
/// wake — is held back. Two determinism properties follow: no bystander
/// epilogue can spawn the reconciliation driver mid-interleave, and no
/// bystander's own context refresh can consume an armed context-install
/// arm. The caller owes exactly one `release_post_refresh_commit` per
/// parked bystander (the gate's release wakes one parked arrival per
/// notify) once the interleave is done, so the tails drain before
/// teardown.
async fn land_unrelated_parked(ladder: &mut Ladder, name: &str, uri: &str, content: &str) {
    ladder.fixture.write_file(name, content).unwrap();
    ry_lsp::test_seam::arm_post_refresh_commit();
    ladder
        .session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": uri, "type": 2}]}),
        )
        .await
        .unwrap();
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_post_refresh_commit(),
    )
    .await
    .expect("the bystander refresh must land and park at the post-commit gate");
}

/// Drive A's ladder to the superseded backstop: attempt 0 loses to B,
/// the retry loses to C, the escalation scan loses to D. Returns with
/// every gate released and SILENCE thereafter — the exact state in
/// which the pre-fix server forgot `a.R` forever.
async fn exhaust_ladder_and_supersede_the_backstop(ladder: &mut Ladder, a_final: &str) {
    park_ladder_backstop(ladder, a_final).await;
    ry_lsp::test_seam::release_scan_commit();
}

/// Force ONE retirement-free driver round: the round's dispatched
/// refresh for `a.R` loses attempt 0 to `b.R`, the retry to `c.R`, and
/// the escalation scan to `d.R` — and, decisively for the stall
/// accounting, every bystander lands through
/// [`land_unrelated_parked`], so its generation bump is sealed while
/// its handler tail (its own context refresh, its acknowledgement,
/// its RETIREMENT) is held parked: a retirement landing inside the
/// round reads as the driver's own progress and resets the stall
/// count, so a streak built with released bystanders never reaches
/// the bound at all. The round's dispatch is the watched event's own
/// refresh when `first` is set (the event is sent here) and the
/// driver's re-drive otherwise — the driver re-claims a fresh epoch
/// on its own, so no event is needed to keep the streak going.
/// Returns with every gate released and the round provably ended
/// without landing and without retiring anything: one round added to
/// the stall count, the obligation retained. The caller owes three
/// `release_post_refresh_commit` calls per round (one per parked
/// bystander) once the streak is over.
async fn stall_one_reconcile_round(ladder: &mut Ladder, a_final: &str, bump: u32, first: bool) {
    park_round_dispatch(ladder, a_final, first).await;
    lose_the_parked_round(ladder, bump).await;
}

/// The [`stall_one_reconcile_round`] prefix: arm the per-file commit
/// gate and rendezvous with the round's dispatched refresh — the
/// watched event's own when `first` (the event is sent here, after the
/// arm), the driver's re-drive otherwise. The arm happens before the
/// preceding round's released scan can make progress: the
/// single-threaded test runtime only advances server tasks across the
/// test's awaits, so the dispatch deterministically parks. Returns
/// with the dispatch parked at the gate; the caller owes the loss
/// chain ([`lose_the_parked_round`]) and, through it, the paired
/// release.
async fn park_round_dispatch(ladder: &mut Ladder, a_final: &str, first: bool) {
    ry_lsp::test_seam::arm_refresh_commit();
    if first {
        ladder.fixture.write_file("a.R", a_final).unwrap();
        ladder
            .session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": ladder.a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
    }
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_refresh_commit(),
    )
    .await
    .expect("the round's dispatched refresh must reach the armed commit gate");
}

/// The [`stall_one_reconcile_round`] suffix, on a dispatch parked by
/// [`park_round_dispatch`]: force it through the full loss chain —
/// attempt 0 loses to `b.R`, the retry to `c.R`, the escalation scan to
/// `d.R` — with every bystander landing parked at its post-commit gate
/// so no retirement can pollute the round. Returns with the scan
/// released and the round folded retirement-free: one round added to
/// the stall count, the obligation retained.
async fn lose_the_parked_round(ladder: &mut Ladder, bump: u32) {
    // First loss: `b.R` lands (bump sealed) while attempt 0 is parked,
    // and stays parked so its retirement cannot pollute the round.
    let b_uri = ladder.b_uri.clone();
    land_unrelated_parked(ladder, "b.R", &b_uri, &format!("o <- {}L\n", bump + 2)).await;

    // Re-arm while attempt 0 is still parked so the retry parks too,
    // then release: attempt 0's commit fails the moved generation and
    // the retry re-reads and parks at the re-armed gate.
    ry_lsp::test_seam::arm_refresh_commit();
    ry_lsp::test_seam::release_refresh_commit();
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_refresh_commit(),
    )
    .await
    .expect("the retry must reach the re-armed commit gate");

    // Second loss: `c.R` lands (parked) while the retry is parked.
    let c_uri = ladder.c_uri.clone();
    land_unrelated_parked(ladder, "c.R", &c_uri, &format!("p <- {}L\n", bump + 2)).await;

    // Arm the scan's commit gate, then release the retry: its commit
    // fails the generation check, the escalation spawns the backstop
    // scan, and the scan parks at its own gate.
    ry_lsp::test_seam::arm_scan_commit();
    ry_lsp::test_seam::release_refresh_commit();
    tokio::time::timeout(rpc_receive_timeout(), ry_lsp::test_seam::wait_scan_commit())
        .await
        .expect("the escalation must spawn the backstop scan");

    // The scan's loss: `d.R` lands (parked) while the scan is parked at
    // its commit, so the scan's generation check will fail when
    // released — the round ends with no landing, no retirement, and
    // the obligation retained.
    let d_uri = ladder.d_uri.clone();
    land_unrelated_parked(ladder, "d.R", &d_uri, &format!("q <- {}L\n", bump + 2)).await;
    ry_lsp::test_seam::release_scan_commit();
}

/// Release every bystander a [`stall_one_reconcile_round`] streak
/// parked (three per round): each release wakes one parked post-commit
/// arrival, whose handler tail then settles its own ROOT group, retires
/// its own obligation, and drains. Sync on purpose — the released tails
/// run across the caller's next await.
fn release_parked_bystanders(rounds: u32) {
    for _ in 0..rounds * 3 {
        ry_lsp::test_seam::release_post_refresh_commit();
    }
}

/// The `exhaust_ladder_and_supersede_the_backstop` prefix, stopping one
/// step earlier: D has landed (the scan's loss is already sealed) but
/// the scan itself is still parked at its commit gate. Callers control
/// the interleaving window between the scan's loss and its release.
async fn park_ladder_backstop(ladder: &mut Ladder, a_final: &str) {
    park_ladder_scan(ladder, a_final).await;

    // The scan's loss: D lands while the scan is parked at its commit,
    // so the scan's generation check will fail when released.
    land_unrelated(ladder, "d.R", &ladder.d_uri.clone(), "q <- 2L\n").await;
}

/// The ladder through its ESCALATION only: attempt 0 loses to B, the
/// retry loses to C, and the backstop scan sits parked at its commit
/// gate with D not yet landed. A's refresh is in flight and its
/// obligation retained — the state a concurrent driver would find
/// undrivable.
async fn park_ladder_scan(ladder: &mut Ladder, a_final: &str) {
    // Fix `a.R` on disk; its watched refresh parks at its commit
    // (attempt 0, post-read).
    ladder.fixture.write_file("a.R", a_final).unwrap();
    ry_lsp::test_seam::arm_refresh_commit();
    ladder
        .session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": ladder.a_uri, "type": 2}]}),
        )
        .await
        .unwrap();
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_refresh_commit(),
    )
    .await
    .expect("attempt 0 must reach the armed commit gate");

    // First loss: B lands while attempt 0 is parked.
    land_unrelated(ladder, "b.R", &ladder.b_uri.clone(), "o <- 2L\n").await;

    // Re-arm while attempt 0 is still parked so the retry parks too,
    // then release: attempt 0's commit fails the moved generation and
    // the retry re-reads and parks at the re-armed gate.
    ry_lsp::test_seam::arm_refresh_commit();
    ry_lsp::test_seam::release_refresh_commit();
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_refresh_commit(),
    )
    .await
    .expect("the retry must reach the re-armed commit gate");

    // Second loss: C lands while the retry is parked.
    land_unrelated(ladder, "c.R", &ladder.c_uri.clone(), "p <- 2L\n").await;

    // Arm the scan's commit gate, then release the retry: its commit
    // fails the generation check, the escalation spawns the backstop
    // scan, and the scan parks at its own gate.
    ry_lsp::test_seam::arm_scan_commit();
    ry_lsp::test_seam::release_refresh_commit();
    tokio::time::timeout(rpc_receive_timeout(), ry_lsp::test_seam::wait_scan_commit())
        .await
        .expect("the escalation must spawn the backstop scan");
}

/// Acceptance 1 + the error-to-clean half of acceptance 2: after the
/// superseded backstop and complete silence, `a.R` must converge to
/// its current (fixed) bytes without any rescue event. Pre-fix, the
/// last `a.R` publication is the stale RY010 from D's republish pass
/// and nothing ever follows it.
#[test]
fn superseded_backstop_then_silence_converges_error_to_clean() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;

        // True-positive control: the pre-ladder tree flags `a.R` in a
        // fresh CLI run, so the post-fix cleanliness below cannot pass
        // vacuously.
        assert!(
            cli_flags(&ladder.fixture, "a.R"),
            "the CLI must flag the pre-fix tree"
        );

        exhaust_ladder_and_supersede_the_backstop(&mut ladder, "x <- 1L\n").await;

        // Silence. D's debounced republish pass still publishes the
        // STALE `a.R` state (its bytes never landed); the reconciliation
        // driver must then land the fix and republish. The quiesce
        // drain keeps the LAST publication per URI, so the assertion
        // fails deterministically pre-fix (stale RY010 survives) and
        // passes post-fix.
        let a_diagnostics = await_diagnostics_where(
            &mut ladder.session,
            &ladder.a_uri,
            |diagnostics| !has_ry010(diagnostics),
            4,
        )
        .await;
        assert!(
            !has_ry010(&a_diagnostics),
            "the retained obligation must converge a.R to its fixed bytes: {a_diagnostics:?}"
        );
        assert!(
            !cli_flags(&ladder.fixture, "a.R"),
            "neighboring valid control: the CLI on the final tree is clean"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// The clean-to-error half of acceptance 2: the same ladder must
/// converge in the opposite direction — a watched edit that INTRODUCES
/// an error reaches the client, not only one that fixes one.
#[test]
fn superseded_backstop_then_silence_converges_clean_to_error() {
    run(async {
        let mut ladder = settled_ladder("x <- 1L\n").await;

        assert!(
            !cli_flags(&ladder.fixture, "a.R"),
            "the CLI must be clean on the pre-edit tree"
        );

        exhaust_ladder_and_supersede_the_backstop(&mut ladder, "x <- never_bound_here\n").await;

        let a_diagnostics =
            await_diagnostics_where(&mut ladder.session, &ladder.a_uri, has_ry010, 4).await;
        assert!(
            has_ry010(&a_diagnostics),
            "the retained obligation must converge a.R to its broken bytes: {a_diagnostics:?}"
        );
        assert!(
            cli_flags(&ladder.fixture, "a.R"),
            "true-positive control: the CLI on the final tree flags a.R"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// Beyond the old drop bound: more than eight finite rounds of
/// supersession, then silence with NO rescue notification — the
/// driver must retain the obligation past its stall bound (pacing
/// itself, not dropping) and converge the repaired closed file to its
/// clean state. Pre-fix, the eighth stalled round purged the map and
/// the stale RY010 stood forever.
#[test]
fn stalled_rounds_beyond_the_pace_bound_retain_and_converge_error_to_clean() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;

        // True-positive control: the pre-ladder tree flags `a.R` in a
        // fresh CLI run, so the post-fix cleanliness below cannot pass
        // vacuously.
        assert!(
            cli_flags(&ladder.fixture, "a.R"),
            "the CLI must flag the pre-fix tree"
        );

        // Ten forced losses — the first belongs to the event's own
        // refresh, so ten are NINE stalled driver rounds, one past the
        // bound — each forced through the full loss chain (attempt 0,
        // retry, backstop scan) with the driver re-driving `a.R` on its
        // own after the first. Rounds at and beyond the bound pace (a
        // bounded backoff before the next dispatch), which the waits
        // below simply absorb.
        for round in 0..10 {
            stall_one_reconcile_round(&mut ladder, "x <- 1L\n", round, round == 0).await;
        }
        release_parked_bystanders(10);

        // Silence: no further events. The next round has nothing left
        // to lose, lands the fix, settles the context, publishes, and
        // retires the obligation.
        let a_diagnostics = await_diagnostics_where(
            &mut ladder.session,
            &ladder.a_uri,
            |diagnostics| !has_ry010(diagnostics),
            6,
        )
        .await;
        assert!(
            !has_ry010(&a_diagnostics),
            "the obligation retained past the stall bound must converge a.R to its fixed bytes: {a_diagnostics:?}"
        );
        assert!(
            !cli_flags(&ladder.fixture, "a.R"),
            "neighboring valid control: the CLI on the final tree is clean"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// The clean-to-error direction of the beyond-the-bound convergence:
/// a newly broken closed file must GAIN its diagnostic after more
/// than eight stalled rounds and silence, not only a repaired one
/// clear it.
#[test]
fn stalled_rounds_beyond_the_pace_bound_retain_and_converge_clean_to_error() {
    run(async {
        let mut ladder = settled_ladder("x <- 1L\n").await;

        assert!(
            !cli_flags(&ladder.fixture, "a.R"),
            "the CLI must be clean on the pre-edit tree"
        );

        for round in 0..10 {
            stall_one_reconcile_round(&mut ladder, "x <- never_bound_here\n", round, round == 0)
                .await;
        }
        release_parked_bystanders(10);

        let a_diagnostics =
            await_diagnostics_where(&mut ladder.session, &ladder.a_uri, has_ry010, 6).await;
        assert!(
            has_ry010(&a_diagnostics),
            "the obligation retained past the stall bound must converge a.R to its broken bytes: {a_diagnostics:?}"
        );
        assert!(
            cli_flags(&ladder.fixture, "a.R"),
            "true-positive control: the CLI on the final tree flags a.R"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// Blocker 1 (version-specific completion): an OLDER refresh's final
/// acknowledgement must not retire a NEWER event's re-armed obligation,
/// even after the newer entry has advanced to the context/publication
/// phase — the exact shape a duty-only acknowledgement mistakes for its
/// own completed phase. The bytes are resolution-sensitive on purpose:
/// `pkgA/a.R` calls `load("data.rda")` (one object, `a`) and uses `a`
/// on both sides of the call, so B's landing owes a real package-group
/// context refresh — the load inventory the check consults is installed
/// state, keyed by the load call's byte offset in the owning parse, and
/// a re-parse alone cannot re-key it. (The checker's search-path
/// leniency after a `load()` call makes the STALE publication's
/// diagnostics coincide with the fresh one's, so the entry-survival
/// assertion cannot ride on diagnostic content; it rides on the
/// re-attempt below, and the closing assertions pin the
/// convergence-to-fresh-analysis agreement on these same
/// resolution-sensitive bytes.)
///
/// The interleave, driven through the gates: A lands and parks AFTER
/// its context settles and its publication is scheduled, BEFORE its
/// acknowledgement; B lands newer bytes (the entry re-arms at B's
/// higher epoch and advances to the context/publication phase), and
/// B's immediate context attempt loses attempt 0 to `b.R`, the retry
/// to `c.R`, and the escalated backstop scan to `d.R` — every
/// bystander left parked at its post-commit gate so no epilogue wake
/// spawns the driver and no bystander tail steals an armed gate. A's
/// parked acknowledgement then resumes while B's scan is still parked:
/// pre-fix it removed B's entry by phase name alone — nothing was left
/// that owed `pkgA`'s context, so no worker ever re-attempts the group
/// and the armed wait for the re-attempt times out. Post-fix the
/// entry's higher epoch keeps it owed, and A's epilogue wake (the
/// first wake nothing parks) spawns the driver to finish B's
/// superseded attempt under silence. The bystanders live in the ROOT
/// package group (no `DESCRIPTION`), so their own tails can never
/// settle `pkgA`'s context — only B's pipeline or the driver can,
/// which is what makes the pre-fix outcome deterministic.
#[test]
fn older_acknowledgement_cannot_retire_a_newer_resolution_obligation() {
    run(async {
        let mut ladder = settled_ladder_in(
            "pkgA/a.R",
            "load(\"data.rda\")\na\n",
            &[
                ("pkgA/DESCRIPTION", b"Package: pkgA\nVersion: 0.0.1\n"),
                (
                    "pkgA/data.rda",
                    include_bytes!("../../ry-testkit/testdata/complete-package/data/ordinary.rda"),
                ),
            ],
        )
        .await;

        assert!(
            !cli_flags(&ladder.fixture, "a.R"),
            "the CLI must be clean on the pre-edit tree (the load sits above the use)"
        );

        // Park A after its context work: the armed acknowledgement gate
        // stops the watched handler between the settled
        // context/publication and the obligation retirement.
        ry_lsp::test_seam::arm_publication_ack();
        ladder
            .fixture
            .write_file("pkgA/a.R", "load(\"data.rda\")\na\na\n")
            .unwrap();
        ladder
            .session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": ladder.a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_publication_ack(),
        )
        .await
        .expect("A's pipeline must park between its settled context and its acknowledgement");

        // B's event lands newer bytes: the entry re-arms at B's epoch and
        // advances to the context/publication phase when the refresh
        // lands. B's bytes move the `load()` call below one use and
        // keep one above it — the after-load use is the context-gated
        // observable (A's stale inventory keys A's byte offset, so the
        // moved call introduces nothing until the group re-resolves).
        // B's immediate context attempt parks at the armed install
        // gate, after its resolve, before the install's generation
        // guard.
        ry_lsp::test_seam::arm_context_install();
        ladder
            .fixture
            .write_file("pkgA/a.R", "a\nload(\"data.rda\")\na\n")
            .unwrap();
        ladder
            .session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": ladder.a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_context_install(),
        )
        .await
        .expect("B's immediate context attempt must park at the install gate");

        // Loss 1: `b.R` lands (generation bumped) while B's attempt 0 is
        // parked, and stays parked itself so its epilogue cannot spawn
        // the driver. The re-armed install gate can then only be
        // consumed by B's ladder retry.
        let b_uri = ladder.b_uri.clone();
        land_unrelated_parked(&mut ladder, "b.R", &b_uri, "o <- 2L\n").await;
        ry_lsp::test_seam::arm_context_install();
        ry_lsp::test_seam::release_context_install();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_context_install(),
        )
        .await
        .expect("B's retry must park at the re-armed install gate");

        // Loss 2: `c.R` lands while the retry is parked. The retry's
        // release then fails the generation guard and escalates the
        // full-scan backstop, which parks at its own commit gate after
        // the walk.
        let c_uri = ladder.c_uri.clone();
        land_unrelated_parked(&mut ladder, "c.R", &c_uri, "p <- 2L\n").await;
        ry_lsp::test_seam::arm_scan_commit();
        ry_lsp::test_seam::release_context_install();
        tokio::time::timeout(rpc_receive_timeout(), ry_lsp::test_seam::wait_scan_commit())
            .await
            .expect("B's escalated backstop scan must park at its commit gate");

        // A's parked acknowledgement resumes — while B's scan is still
        // parked and every bystander tail with it, so no driver exists
        // yet. The context gate is armed FIRST so the driver's own
        // `pkgA` attempt (the only context attempt left anywhere: B's
        // ladder is exhausted behind its parked scan, and the parked
        // bystanders owe only their ROOT group) deterministically parks
        // when it runs. Pre-fix the removal is the bug: the duty name
        // matched A's completed phase, so B's entry vanished with the
        // group's context still owed, no worker ever re-attempts
        // `pkgA`, and the armed wait below times out. Post-fix the
        // entry's higher epoch keeps it owed, and A's epilogue wake
        // (the first wake nothing parks) spawns the driver to finish
        // it — the observable form of "B's entry survives A's
        // acknowledgement".
        ry_lsp::test_seam::arm_context_install();
        ry_lsp::test_seam::release_publication_ack();
        // The scan's loss: `d.R` lands (the generation moves again) while
        // the scan is parked at its commit, sealing its superseded
        // verdict — and defeating the driver's parked first install
        // too, whose ladder retry then settles: exactly the
        // supersession chain the entry's survival is for.
        let d_uri = ladder.d_uri.clone();
        land_unrelated_parked(&mut ladder, "d.R", &d_uri, "q <- 2L\n").await;
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_context_install(),
        )
        .await
        .expect("the surviving obligation's driver must re-attempt pkgA's context after A's acknowledgement");
        ry_lsp::test_seam::release_scan_commit();
        // Release the parked driver attempt: its install loses the
        // generation guard to `d.R`'s bump, the retry re-snapshots and
        // installs, the driver publishes and retires the entry at B's
        // own epoch.
        ry_lsp::test_seam::release_context_install();
        // Release the three parked bystanders: their tails settle their
        // own ROOT group, acknowledge their own obligations, and drain —
        // none of which can advance `pkgA`'s context.
        ry_lsp::test_seam::release_post_refresh_commit();
        ry_lsp::test_seam::release_post_refresh_commit();
        ry_lsp::test_seam::release_post_refresh_commit();

        // Silence: no further events. Pre-fix, every publication pairs
        // B's bytes with A's stale inventory — the moved `load()` call
        // introduces nothing, so the after-load use keeps firing and
        // nothing is left that owes `pkgA`'s context. Post-fix the
        // driver converges the group: the fresh inventory keys B's own
        // byte offset, the after-load use resolves again, and only the
        // use above the load reports.
        let a_diagnostics = await_diagnostics_where(
            &mut ladder.session,
            &ladder.a_uri,
            |diagnostics| {
                has_ry010_for_a_at_line(diagnostics, 0) && !has_ry010_for_a_at_line(diagnostics, 2)
            },
            8,
        )
        .await;
        assert!(
            has_ry010_for_a_at_line(&a_diagnostics, 0),
            "the use above the load() call must report against the fresh inventory too: {a_diagnostics:?}"
        );
        assert!(
            !has_ry010_for_a_at_line(&a_diagnostics, 2),
            "B's surviving obligation must re-key the load inventory so the after-load use resolves: {a_diagnostics:?}"
        );
        assert!(
            cli_flags(&ladder.fixture, "a.R"),
            "true-positive control: the CLI on the final tree flags exactly the use above the load() call"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// Blocker 2 (paced retries, pinned rung by rung): a driver whose
/// rounds keep RETURNING unsuccessfully — each losing its ladder to
/// sealed bystander bumps, folding, and owing the next round — must
/// not begin that next round before its pace rung expires. Nine forced
/// losses (the first the event's own refresh) are eight stalled driver
/// rounds: the eighth fold retains the obligation and enters the paced
/// regime. Rounds nine through twelve then each park their dispatch,
/// lose the full chain, and fold; every dispatch rendezvouses at the
/// armed commit gate, and the interval from the PREVIOUS round's scan
/// release to that rendezvous — inside which the fold, the pace sleep,
/// and the next round's head and read all run — must cover the full
/// rung (`reconcile_pace_delay`: 40ms doubled per further stalled
/// round, capped at 320ms). The bounds are one-sided on purpose:
/// scheduler noise can only LENGTHEN a measured interval, so wall
/// clock needs no mock here — with the pacing sleep removed, the next
/// round begins after only its own milliseconds of work and the first
/// rung's bound fails, which is precisely the mutation this test
/// exists to catch (a window that watches a PARKED dispatch proves
/// nothing about the wait between rounds: the parked dispatch blocks
/// its own round's fold, so the single driver cannot advance even
/// unpaced). The scan counter carries the no-rescan half across
/// rounds that DO escalate: four stalled rounds, exactly four ladder
/// backstops — never a background pass per paced retry. Releasing the
/// parked bystanders makes completion possible again, and the retained
/// obligation converges under silence.
#[test]
fn paced_rounds_cannot_begin_before_their_backoff_expires() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;

        assert!(
            cli_flags(&ladder.fixture, "a.R"),
            "the CLI must flag the pre-fix tree"
        );

        // Nine forced losses — the first belongs to the event's own
        // refresh, so nine are EIGHT stalled driver rounds, entering
        // the paced regime: the eighth fold retains the obligation and
        // yields for the first rung before round nine.
        for round in 0..9 {
            stall_one_reconcile_round(&mut ladder, "x <- 1L\n", round, round == 0).await;
        }

        let scans_before = ry_lsp::test_seam::background_index_spawns();

        // Rounds nine through twelve: time each dispatch's rendezvous
        // from the previous round's release (the timer starts before
        // the arm, with no await in between, so the previous fold and
        // its pace sleep fall inside the measured window), then lose
        // the round — one more retirement-free fold, the next rung
        // grown. The rung values mirror `reconcile_pace_delay`.
        let rungs_ms = [40u64, 80, 160, 320];
        for (index, rung_ms) in rungs_ms.into_iter().enumerate() {
            let started = std::time::Instant::now();
            park_round_dispatch(&mut ladder, "x <- 1L\n", false).await;
            let elapsed = started.elapsed();
            assert!(
                elapsed >= std::time::Duration::from_millis(rung_ms),
                "round {} must not begin before its {rung_ms}ms pace rung expires; the dispatch arrived after only {elapsed:?}",
                9 + index,
            );
            lose_the_parked_round(&mut ladder, 9 + index as u32).await;
        }

        // The no-rescan bound, across rounds that do escalate: four
        // stalled rounds spawned exactly their four ladder backstops,
        // and no paced retry spawned anything.
        assert_eq!(
            ry_lsp::test_seam::background_index_spawns() - scans_before,
            rungs_ms.len(),
            "a stalled round may escalate only its own ladder backstop — never a background pass per paced retry"
        );

        // Completion becomes possible again: nothing has moved the
        // generation since the twelfth round's last loss, so the next
        // paced dispatch lands, and the retained obligation converges
        // without any rescue event.
        release_parked_bystanders(13);
        let a_diagnostics = await_diagnostics_where(
            &mut ladder.session,
            &ladder.a_uri,
            |diagnostics| !has_ry010(diagnostics),
            6,
        )
        .await;
        assert!(
            !has_ry010(&a_diagnostics),
            "the obligation retained through the paced episode must converge a.R once completion is possible again: {a_diagnostics:?}"
        );
        assert!(
            !cli_flags(&ladder.fixture, "a.R"),
            "neighboring valid control: the CLI on the final tree is clean"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// Acceptance 11 (event-during-in-flight-work half): an event arriving
/// while watched work is in flight — here, while A's escalation scan
/// sits parked at its commit gate — must be processed without a later
/// wakeup and without disturbing the in-flight work's own convergence.
/// The wakeup coordination this exercises is the enqueue-before-read /
/// idle-transition-under-lock design: B's event enqueues its own
/// obligation through its refresh claim, B's handler epilogue wakes
/// (or finds) the one driver, and BOTH B's event and A's retained
/// obligation converge. The driver may legitimately take over A's
/// obligation mid-ladder by claiming a newer epoch (#538's start-order
/// rule applied to the bookkeeping), so the test asserts convergence
/// of both paths rather than which worker landed them.
#[test]
fn event_during_in_flight_watched_work_is_not_lost() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;

        // Inline ladder: attempt 0 loses to C, the retry loses to D,
        // and B's event is forwarded while the escalation scan is
        // parked (the in-flight window).
        ladder.fixture.write_file("a.R", "x <- 1L\n").unwrap();
        ry_lsp::test_seam::arm_refresh_commit();
        ladder
            .session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": ladder.a_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_refresh_commit(),
        )
        .await
        .expect("attempt 0 must reach the armed commit gate");
        let c_uri = ladder.c_uri.clone();
        land_unrelated(&mut ladder, "c.R", &c_uri, "p <- 2L\n").await;

        ry_lsp::test_seam::arm_refresh_commit();
        ry_lsp::test_seam::release_refresh_commit();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_refresh_commit(),
        )
        .await
        .expect("the retry must reach the re-armed commit gate");
        let d_uri = ladder.d_uri.clone();
        land_unrelated(&mut ladder, "d.R", &d_uri, "q <- 2L\n").await;

        ry_lsp::test_seam::arm_scan_commit();
        ry_lsp::test_seam::release_refresh_commit();
        tokio::time::timeout(rpc_receive_timeout(), ry_lsp::test_seam::wait_scan_commit())
            .await
            .expect("the escalation must spawn the backstop scan");

        // B's event arrives while the scan is parked mid-flight, its
        // bytes carrying an independently checkable error.
        ladder
            .fixture
            .write_file("b.R", "o <- never_bound_midflight\n")
            .unwrap();
        ladder
            .session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": ladder.b_uri, "type": 2}]}),
            )
            .await
            .unwrap();

        // Release the scan (it loses to D's landing), then silence.
        ry_lsp::test_seam::release_scan_commit();

        let a_diagnostics = await_diagnostics_where(
            &mut ladder.session,
            &ladder.a_uri,
            |diagnostics| !has_ry010(diagnostics),
            4,
        )
        .await;
        assert!(
            !has_ry010(&a_diagnostics),
            "a's retained obligation must converge to its fixed bytes: {a_diagnostics:?}"
        );
        let b_diagnostics =
            await_diagnostics_where(&mut ladder.session, &ladder.b_uri, has_ry010, 4).await;
        assert!(
            has_ry010(&b_diagnostics),
            "b's event during in-flight watched work must not be lost: {b_diagnostics:?}"
        );
        assert!(
            cli_flags(&ladder.fixture, "b.R"),
            "true-positive control: the CLI flags b.R on the final tree"
        );

        // One driver at a time serves the whole dance; the early-driver
        // takeover can legitimately respawn after an idle transition,
        // so the bound allows a couple of (re)spawns — but never one
        // per event.
        let spawns = ry_lsp::test_seam::reconciliation_driver_spawns();
        assert!(spawns <= 3, "driver spawns ({spawns}) must stay coalesced");

        join_session(ladder.session, ladder.server).await;
    })
}

/// Acceptance 10: a burst of events over a small path set coalesces.
/// The happy path first — a single unopposed event converges with ZERO
/// driver activity (the driver is pure overhead until something is
/// actually retained) — then a twelve-event burst over two paths whose
/// final bytes must all reach the client.
#[test]
fn event_burst_coalesces_and_converges() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;

        // Happy path: no losses are possible with nothing concurrent,
        // so nothing may be retained and no driver may run. The
        // observation is scoped, not process-global: the counter mark
        // is taken after `settled_ladder` proved the close handler's
        // whole tail finished (its empty-clear publication), so
        // earlier session phases cannot pollute the delta; and the
        // event's landing is proven through the post-commit gate
        // rather than a timing window. Zero spawns is then structural:
        // the landed refresh's exit leaves only the dispatcher-owned
        // context/publication duty (the exit-side wake skips it), and
        // the handler completes the obligation BEFORE its epilogue
        // wake, so the wake — whenever it runs — finds an empty map.
        let spawns_before = ry_lsp::test_seam::reconciliation_driver_spawns();
        let rounds_before = ry_lsp::test_seam::reconciliation_rounds();
        ladder.fixture.write_file("b.R", "o <- 2L\n").unwrap();
        ry_lsp::test_seam::arm_post_refresh_commit();
        ladder
            .session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": ladder.b_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_post_refresh_commit(),
        )
        .await
        .expect("the unopposed event's refresh must land");
        ry_lsp::test_seam::release_post_refresh_commit();
        let b_diagnostics =
            await_diagnostics_where(&mut ladder.session, &ladder.b_uri, |d| !has_ry010(d), 4).await;
        assert!(!has_ry010(&b_diagnostics), "the single event must converge");
        assert_eq!(
            ry_lsp::test_seam::reconciliation_driver_spawns(),
            spawns_before,
            "an unopposed event retains nothing: no driver may spawn"
        );
        assert_eq!(
            ry_lsp::test_seam::reconciliation_rounds(),
            rounds_before,
            "an unopposed event drives no reconciliation rounds"
        );

        // Burst: twelve events over two paths, final states one clean
        // and one broken, all delivered back-to-back so the handlers
        // interleave freely.
        for index in 0..6 {
            ladder
                .fixture
                .write_file("b.R", format!("o <- {index}L\n"))
                .unwrap();
            ladder
                .session
                .notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ladder.b_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            ladder
                .fixture
                .write_file("c.R", format!("p <- {index}L\n"))
                .unwrap();
            ladder
                .session
                .notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ladder.c_uri, "type": 2}]}),
                )
                .await
                .unwrap();
        }
        ladder
            .fixture
            .write_file("b.R", "o <- never_bound_final\n")
            .unwrap();
        ladder
            .session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": ladder.b_uri, "type": 2}]}),
            )
            .await
            .unwrap();

        let b_diagnostics =
            await_diagnostics_where(&mut ladder.session, &ladder.b_uri, has_ry010, 6).await;
        assert!(
            has_ry010(&b_diagnostics),
            "the burst's final broken b.R must reach the client"
        );
        let c_diagnostics =
            await_diagnostics_where(&mut ladder.session, &ladder.c_uri, |d| !has_ry010(d), 2).await;
        assert!(
            !has_ry010(&c_diagnostics),
            "the burst's final clean c.R must reach the client"
        );
        // Structural coalescing bound: a wake spawns a driver only
        // when none is active, so spawns never exceed handler
        // epilogues (thirteen events above) regardless of interleaving.
        let spawns = ry_lsp::test_seam::reconciliation_driver_spawns();
        assert!(
            spawns <= 13,
            "driver spawns ({spawns}) must stay bounded by event count"
        );
        assert!(
            cli_flags(&ladder.fixture, "b.R") && !cli_flags(&ladder.fixture, "c.R"),
            "the CLI oracle agrees on the final tree"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// Acceptance 12: shutdown with a retained (queued) obligation must
/// terminate cleanly — the shutdown epilogue cancels the obligation
/// and no later publication resurrects through the dropped runtime.
#[test]
fn teardown_with_a_retained_obligation_terminates() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;
        exhaust_ladder_and_supersede_the_backstop(&mut ladder, "x <- 1L\n").await;
        // Silence, then immediate teardown: the obligation is queued
        // (and the driver may be mid-round) as the session ends.
        join_session(ladder.session, ladder.server).await;
    })
}

/// The idle-transition wake-retention interleaving: a driver whose
/// round finds only in-flight work must not drop the signal when an
/// in-flight refresh exits inside the window between the round's last
/// duty check and the flag clear. The interleave, driven through the
/// gates: A's escalation scan parks (A's refresh in flight, its
/// obligation retained), the idle gate is armed, D's event lands — its
/// handler's epilogue wake spawns a driver whose round finds only the
/// in-flight A and parks at the idle gate — and only THEN is A's
/// losing scan released: its exit-side and epilogue wakes find the
/// active flag and wake nobody. Pre-fix, the parked idle decision
/// cleared the flag and returned, stranding A's retained obligation
/// under silence; post-fix it re-derives its verdict under the same
/// lock hold and keeps driving.
#[test]
fn driver_idle_out_keeps_the_signal_of_a_refresh_exiting_in_the_window() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;

        // Inline ladder through the parked escalation: attempt 0 loses
        // to B, the retry loses to C, and the backstop scan parks with
        // D not yet landed — A's refresh in flight, undrivable.
        park_ladder_scan(&mut ladder, "x <- 1L\n").await;

        // Arm the idle gate BEFORE the wake that spawns the driver, so
        // the driver's idle decision deterministically parks.
        ry_lsp::test_seam::arm_driver_idle();

        // D's event lands (sealing the scan's loss): its handler
        // completes and its epilogue wake spawns the driver, whose
        // first round finds only the in-flight A and parks at the
        // armed idle decision.
        let d_uri = ladder.d_uri.clone();
        land_unrelated(&mut ladder, "d.R", &d_uri, "q <- 2L\n").await;
        tokio::time::timeout(rpc_receive_timeout(), ry_lsp::test_seam::wait_driver_idle())
            .await
            .expect("the driver must reach its armed idle decision");

        // Release the scan: its commit fails the moved generation, A's
        // refresh exits RETAINED (the obligation survives it, idle),
        // and both its exit-side wake and its handler's epilogue wake
        // find the driver's active flag — waking nobody. This is the
        // lost-wake window the idle decision's re-check must survive.
        ry_lsp::test_seam::release_scan_commit();

        // Release the idle decision: post-fix it re-checks under the
        // clearing lock, finds A's obligation drivable, and keeps
        // driving; the re-drive lands the fixed bytes and publishes.
        ry_lsp::test_seam::release_driver_idle();

        let a_diagnostics = await_diagnostics_where(
            &mut ladder.session,
            &ladder.a_uri,
            |diagnostics| !has_ry010(diagnostics),
            4,
        )
        .await;
        assert!(
            !has_ry010(&a_diagnostics),
            "the exit inside the idle window must not strand the retained obligation: {a_diagnostics:?}"
        );
        assert!(
            !cli_flags(&ladder.fixture, "a.R"),
            "neighboring valid control: the CLI on the final tree is clean"
        );

        join_session(ladder.session, ladder.server).await;
    })
}

/// Shutdown while the driver is provably INSIDE a round: the driver's
/// own re-drive of the retained obligation sits parked at the armed
/// commit gate when the shutdown request is answered, so clearing
/// `pending_refreshes` alone (the pre-fix behavior) cannot help — the
/// round would still follow its landed refresh with client
/// publications. Post-fix, the in-round shutdown check cancels the
/// follow-up: no publication for the parked path may arrive after the
/// shutdown response, and the session still tears down cleanly.
#[test]
fn shutdown_during_a_driver_round_publishes_nothing_afterwards() {
    run(async {
        let mut ladder = settled_ladder("x <- never_bound_here\n").await;

        // Exhaust the ladder, then arm the per-file commit gate BEFORE
        // releasing the scan: the next refresh to reach the gate is the
        // driver's own re-drive of A's retained obligation, parking the
        // driver mid-round with its dispatch in flight.
        park_ladder_backstop(&mut ladder, "x <- 1L\n").await;
        ry_lsp::test_seam::arm_refresh_commit();
        ry_lsp::test_seam::release_scan_commit();
        tokio::time::timeout(
            rpc_receive_timeout(),
            ry_lsp::test_seam::wait_refresh_commit(),
        )
        .await
        .expect("the driver's re-drive must park at the armed commit gate");

        // Drain D's debounced republish — it still carries a.R's stale
        // state, and it was scheduled before shutdown — so the negative
        // window below observes only post-shutdown traffic.
        let stale = await_diagnostics_where(&mut ladder.session, &ladder.a_uri, has_ry010, 4).await;
        assert!(
            has_ry010(&stale),
            "the pre-shutdown republish must carry the stale state (the control for the window)"
        );
        sync_barrier(
            &mut ladder.session,
            &file_uri(&ladder.fixture.path("main.R")),
        )
        .await;
        let mark = ladder.session.publication_mark();

        // Shutdown is answered while the driver's dispatched refresh
        // sits at the gate: the driver is inside its round. The raw
        // request/response pair, NOT `LspSession::shutdown` (which also
        // sends `exit` and would end the server task — closing the
        // socket and suppressing any publication regardless of the
        // fix): the server keeps serving until `exit` below, so the
        // negative window really exercises the in-round checks. The
        // without-params form because tower-lsp rejects an explicit
        // `null` params field on no-parameter methods.
        ladder
            .session
            .request_without_params("shutdown")
            .await
            .unwrap();

        // Release the parked refresh: its bytes may still land (the
        // read predates shutdown), but the round's follow-up
        // publications belong to the ended session and must not go out.
        ry_lsp::test_seam::release_refresh_commit();
        let leaked = tokio::time::timeout(
            std::time::Duration::from_millis(700),
            ladder
                .session
                .published_diagnostics_after(&ladder.a_uri, mark),
        )
        .await;
        assert!(
            leaked.is_err(),
            "no publication for the parked path may follow shutdown; got {:?}",
            leaked.ok()
        );

        // End the session the protocol way: `exit` after the window,
        // then close the stream and join the server.
        ladder.session.notify("exit", Value::Null).await.unwrap();
        drop(ladder.session);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), ladder.server).await;
    })
}
