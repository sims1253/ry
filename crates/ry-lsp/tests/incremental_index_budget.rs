//! #525: incremental disk refreshes must preserve the `index.max-files`
//! budget.
//!
//! Full discovery enforces `index.max-files` as a hard per-root budget,
//! but the per-file refresh path used to insert unconditionally, so
//! watched-file events could grow the index past the cap without bound.
//! The commit now refuses a *new* entry once the owning root is at its
//! budget (first-come-first-served, like the walk): refreshing an
//! already-indexed path still lands, and nothing indexed is evicted.
//!
//! The observable is name resolution through a file the capped walk
//! omits: `use2.R` reads the unbound name `never_bound_here` (one RY010),
//! and a watched file appearing outside the editor defines it. Past the
//! cap the definition must stay out and the RY010 must survive.
//!
//! There is deliberately no fresh-server oracle here: above the cap the
//! walk keeps the first files in readdir order, which is
//! filesystem-dependent, so a fresh server over the final tree may keep a
//! different subset. These tests pin the live invariant instead — the
//! index never grows past the cap — which is exactly what the fix
//! guarantees and what fails without it. The observation rendezvouses
//! with the refresh commit through the `#526` commit gate (a bare
//! round-trip would not prove the one-way watched notification was
//! processed, let alone committed): released plus landed proves the
//! commit decision precedes the no-op edit whose publication is asserted.

mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

/// `use2.R` reads the unbound name: exactly one RY010 while no indexed
/// file defines it.
const USE2: &str = "x <- never_bound_here\n";
/// A definition of that name which, once indexed, would clear the RY010.
const NEWDEFS: &str = "never_bound_here <- function() 1L\n";

fn count_ry010(publish: &Value) -> usize {
    normalize_diagnostics(publish)
        .iter()
        .filter(|diagnostic| diagnostic["code"] == json!("RY010"))
        .count()
}

fn has_ry040(publish: &Value) -> bool {
    normalize_diagnostics(publish)
        .iter()
        .any(|diagnostic| diagnostic["code"] == json!("RY040"))
}

/// Deliver a watched creation event for `uri`.
async fn notify_created(session: &mut harness::ClientSession, uri: &str) {
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": uri, "type": 1}]}),
        )
        .await
        .unwrap();
}

/// Deliver a watched change event for `uri`.
async fn notify_changed(session: &mut harness::ClientSession, uri: &str) {
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": uri, "type": 2}]}),
        )
        .await
        .unwrap();
}

/// Drive one fresh publication of the committed index with a no-op edit
/// and return it. The caller must have rendezvoused with every in-flight
/// writer first, so the snapshot postdates their commit decisions.
async fn observe_current_index(
    session: &mut harness::ClientSession,
    use2_uri: &str,
    version: i32,
) -> Value {
    observe_current_index_with(session, use2_uri, version, USE2).await
}

/// Drive one fresh publication of the committed index with a no-op edit
/// carrying `text`, and return it. Same rendezvous contract as
/// [`observe_current_index`], for observers whose buffer text differs.
async fn observe_current_index_with(
    session: &mut harness::ClientSession,
    uri: &str,
    version: i32,
    text: &str,
) -> Value {
    let mark = session.publication_mark();
    session
        .change(uri, version, json!([{ "text": text }]))
        .await
        .unwrap();
    session
        .published_diagnostics_after(uri, mark)
        .await
        .unwrap()
}

/// Open `use2.R` and return its first publication: publications are gated
/// on the initial index, so this also proves the initial scan settled.
async fn open_use2_settled(session: &mut harness::ClientSession, use2_uri: &str) -> Value {
    let mark = session.publication_mark();
    session.open(use2_uri, 1, USE2).await.unwrap();
    session
        .published_diagnostics_after(use2_uri, mark)
        .await
        .unwrap()
}

/// Deliver the watched event for `uri` (created or changed per `notify`)
/// and rendezvous with its refresh commit through the gate: when this
/// returns, the commit decision — landed or refused — precedes.
async fn notify_and_rendezvous(session: &mut harness::ClientSession, uri: &str, created: bool) {
    ry_lsp::test_seam::arm_refresh_commit();
    if created {
        notify_created(session, uri).await;
    } else {
        notify_changed(session, uri).await;
    }
    ry_lsp::test_seam::wait_refresh_commit().await;
    ry_lsp::test_seam::release_refresh_commit();
    ry_lsp::test_seam::wait_refresh_landed().await;
}

/// A watched creation past the cap must not grow the index: with two
/// files indexed under `max-files = 2`, a third file's definitions stay
/// invisible and the RY010 survives.
#[test]
fn create_past_cap_is_refused() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-files = 2\n")
            .unwrap();
        fixture.write_file("a.R", "f <- function() 1L\n").unwrap();
        fixture.write_file("b.R", "g <- function() 2L\n").unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &a_uri).await;

        // The initial scan holds exactly the two files, at the cap; the
        // observer opens before the over-budget file exists.
        fixture.write_file("use2.R", USE2).unwrap();
        let use2_uri = file_uri(&fixture.path("use2.R"));
        let first = open_use2_settled(&mut session, &use2_uri).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "the name must be unbound before d.R exists: {first}"
        );

        fixture.write_file("d.R", NEWDEFS).unwrap();
        let d_uri = file_uri(&fixture.path("d.R"));
        notify_and_rendezvous(&mut session, &d_uri, true).await;
        let after = observe_current_index(&mut session, &use2_uri, 2).await;
        assert_eq!(
            count_ry010(&after),
            1,
            "d.R is past the max-files budget and must stay out of the index: {after}"
        );

        join_session(session, server).await;
    });
}

/// A watched change to a file the capped walk omitted must not grow the
/// index either: `d.R` starts oversized (deterministically omitted,
/// whatever the readdir order), is then rewritten small, and the change
/// event still refuses it while the root is at the cap.
#[test]
fn change_of_undiscovered_file_past_cap_is_refused() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-files = 2\nmax-file-bytes = 64\n")
            .unwrap();
        fixture.write_file("a.R", "f <- function() 1L\n").unwrap();
        fixture.write_file("b.R", "g <- function() 2L\n").unwrap();
        fixture.write_file("d.R", "y <- 2\n".repeat(30)).unwrap();
        assert!(
            std::fs::metadata(fixture.path("d.R")).unwrap().len() > 64,
            "d.R must start over the size cap so the initial scan omits it"
        );
        let a_uri = file_uri(&fixture.path("a.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &a_uri).await;

        fixture.write_file("use2.R", USE2).unwrap();
        let use2_uri = file_uri(&fixture.path("use2.R"));
        let first = open_use2_settled(&mut session, &use2_uri).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "the name must be unbound before d.R shrinks: {first}"
        );

        // Shrink `d.R` under the size cap and deliver the watched change:
        // admission would now pass, but the root is at its file budget.
        std::fs::write(fixture.path("d.R"), NEWDEFS).unwrap();
        let d_uri = file_uri(&fixture.path("d.R"));
        notify_and_rendezvous(&mut session, &d_uri, false).await;
        let after = observe_current_index(&mut session, &use2_uri, 2).await;
        assert_eq!(
            count_ry010(&after),
            1,
            "d.R is past the max-files budget and must stay out of the index: {after}"
        );

        join_session(session, server).await;
    });
}

/// The budget must not freeze refreshes of indexed files: with the root
/// at the cap, a watched change to an already-indexed file still lands
/// (no growth, so no refusal). This passes without the fix too — it
/// guards the update path against over-refusal.
#[test]
fn refresh_of_indexed_file_at_cap_still_lands() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // `a.R` alone defines the character `f`, so `use.R` carries RY040.
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-files = 2\n")
            .unwrap();
        fixture
            .write_file("a.R", "f <- function() \"str\"\n")
            .unwrap();
        fixture.write_file("use.R", "x <- f() + 1L\n").unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        session.open(&use_uri, 1, "x <- f() + 1L\n").await.unwrap();
        let first = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            has_ry040(&first),
            "character f must win before the on-disk edit: {first}"
        );

        // Rewrite the indexed `a.R` and deliver its watched change: an
        // update, not growth, so the budget must not refuse it. The landed
        // refresh republishes, which is the observation — no gate needed.
        std::fs::write(fixture.path("a.R"), "f <- function() 1L\n").unwrap();
        let edit_mark = session.publication_mark();
        notify_changed(&mut session, &a_uri).await;
        let after = session
            .published_diagnostics_after(&use_uri, edit_mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&after),
            "refreshing the indexed a.R at the cap must still land: {after}"
        );

        join_session(session, server).await;
    });
}

/// Observer buffer inside the inner root: it is created after the
/// settle proof, so it never enters the index, and opening it observes
/// the inner scope (open documents never consume budget).
const OBSERVER_INNER: &str = "y <- inner_marker\n";
/// A definition under the inner root, under its own name.
const INNERDEFS: &str = "inner_marker <- function() 1L\n";
/// A pre-existing inner file so the inner root is non-empty at the
/// initial scan; it shares the outer path prefix while attributing to
/// the inner root.
const KEEP: &str = "keep <- 1L\n";

/// Nested roots must not consume each other's budget (#525): the outer
/// root holds two of its four budgeted files and the inner root holds
/// one of its own, so a new OUTER-owned file still lands — the inner
/// entries share the path prefix but attribute to the inner root. A
/// plain prefix count sees the map at the cap and wrongly refuses it.
#[test]
fn nested_inner_entries_do_not_consume_outer_budget() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-files = 4\n")
            .unwrap();
        // Everything the budget counts is created BEFORE the session
        // starts, so the initial map holds exactly these three files
        // regardless of scan timing: two outer-owned, one inner-owned.
        fixture.write_file("a.R", "f <- function() 1L\n").unwrap();
        fixture.write_file("use2.R", USE2).unwrap();
        std::fs::create_dir(fixture.path("pkg")).unwrap();
        fixture.write_file("pkg/keep.R", KEEP).unwrap();
        let pkg = fixture.path("pkg");
        // Nested workspace: the outer root and `pkg` below it are both
        // folder roots, so the inner scan caps independently while its
        // entries share the outer path prefix in the one `disk_files` map.
        let (mut session, server) = spawn_session(&[fixture.root(), &pkg], json!({}), None).await;
        let a_uri = file_uri(&fixture.path("a.R"));
        sync_barrier(&mut session, &a_uri).await;

        // The initial scan settled (publications gate on it): both names
        // start unbound.
        let use2_uri = file_uri(&fixture.path("use2.R"));
        let first = open_use2_settled(&mut session, &use2_uri).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "the outer name must be unbound before c.R exists: {first}"
        );

        // The inner observer is created after the settle proof, so it
        // never enters the index; opening it observes the inner scope.
        fixture.write_file("pkg/obs.R", OBSERVER_INNER).unwrap();
        let obs_uri = file_uri(&fixture.path("pkg/obs.R"));
        let obs_mark = session.publication_mark();
        session.open(&obs_uri, 1, OBSERVER_INNER).await.unwrap();
        let obs_first = session
            .published_diagnostics_after(&obs_uri, obs_mark)
            .await
            .unwrap();
        assert_eq!(
            count_ry010(&obs_first),
            1,
            "the inner name must be unbound before b.R exists: {obs_first}"
        );

        // Inner-owned creation lands under the inner root's own budget
        // (one of four), which the inner observer proves.
        fixture.write_file("pkg/b.R", INNERDEFS).unwrap();
        let b_uri = file_uri(&fixture.path("pkg/b.R"));
        notify_and_rendezvous(&mut session, &b_uri, true).await;
        let inner_after =
            observe_current_index_with(&mut session, &obs_uri, 2, OBSERVER_INNER).await;
        assert_eq!(
            count_ry010(&inner_after),
            0,
            "b.R is within the inner root's own budget and must land: {inner_after}"
        );

        // Outer-owned creation: the outer root's own subset is two of
        // four, so this must land even though the prefix count under
        // the outer root (two outer files plus inner keep.R and b.R)
        // sits at the cap.
        fixture.write_file("c.R", NEWDEFS).unwrap();
        let c_uri = file_uri(&fixture.path("c.R"));
        notify_and_rendezvous(&mut session, &c_uri, true).await;
        let after = observe_current_index(&mut session, &use2_uri, 2).await;
        assert_eq!(
            count_ry010(&after),
            0,
            "c.R is within the outer root's own budget and must land: {after}"
        );

        join_session(session, server).await;
    });
}
