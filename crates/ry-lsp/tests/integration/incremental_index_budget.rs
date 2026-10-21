//! Watched-file refreshes keep the `index.max-files` budget: a new file is
//! refused once its root is at the cap, and an indexed file still
//! refreshes. `use2.R` reads the unbound `never_bound_here`, and a file
//! created outside the editor defines it.
//!
//! There is no fresh-server oracle: above the cap the walk keeps files in
//! readdir order, which depends on the filesystem. The index writer applies
//! work in arrival order, so a no-op edit after a watched event publishes
//! the state after that event.

use crate::harness;

use harness::{
    change_published, count_code, file_uri, has_ry040, join_session, notify_changed,
    notify_created, open_published, spawn_session, sync_barrier,
};
use ry_testkit::FixtureProject;
use serde_json::json;

/// `use2.R` reads the unbound name: exactly one RY010 while no indexed
/// file defines it.
const USE2: &str = "x <- never_bound_here\n";
/// A definition of that name which, once indexed, would clear the RY010.
const NEWDEFS: &str = "never_bound_here <- function() 1L\n";

/// A watched creation past the cap must not grow the index: with two
/// files indexed under `max-files = 2`, a third file's definitions stay
/// invisible and the RY010 survives.
#[tokio::test]
async fn create_past_cap_is_refused() {
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
    let first = open_published(&mut session, &use2_uri, 1, USE2).await;
    assert_eq!(
        count_code(&first, "RY010"),
        1,
        "the name must be unbound before d.R exists: {first}"
    );

    fixture.write_file("d.R", NEWDEFS).unwrap();
    let d_uri = file_uri(&fixture.path("d.R"));
    notify_created(&mut session, &d_uri).await;
    let after = change_published(&mut session, &use2_uri, 2, USE2).await;
    assert_eq!(
        count_code(&after, "RY010"),
        1,
        "d.R is past the max-files budget and must stay out of the index: {after}"
    );

    join_session(session, server).await;
}

/// A watched change to a file the capped walk omitted must not grow the
/// index either: `d.R` starts oversized (deterministically omitted,
/// whatever the readdir order), is then rewritten small, and the change
/// event still refuses it while the root is at the cap.
#[tokio::test]
async fn change_of_undiscovered_file_past_cap_is_refused() {
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
    let first = open_published(&mut session, &use2_uri, 1, USE2).await;
    assert_eq!(
        count_code(&first, "RY010"),
        1,
        "the name must be unbound before d.R shrinks: {first}"
    );

    // Shrink `d.R` under the size cap and deliver the watched change:
    // admission would now pass, but the root is at its file budget.
    std::fs::write(fixture.path("d.R"), NEWDEFS).unwrap();
    let d_uri = file_uri(&fixture.path("d.R"));
    notify_changed(&mut session, &d_uri).await;
    let after = change_published(&mut session, &use2_uri, 2, USE2).await;
    assert_eq!(
        count_code(&after, "RY010"),
        1,
        "d.R is past the max-files budget and must stay out of the index: {after}"
    );

    join_session(session, server).await;
}

/// The budget must not freeze refreshes of indexed files: with the root
/// at the cap, a watched change to an already-indexed file still lands
/// (no growth, so no refusal). This passes without the fix too — it
/// guards the update path against over-refusal.
#[tokio::test]
async fn refresh_of_indexed_file_at_cap_still_lands() {
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

    let first = open_published(&mut session, &use_uri, 1, "x <- f() + 1L\n").await;
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

/// Nested roots must not consume each other's budget: the outer
/// root holds two of its four budgeted files and the inner root holds
/// one of its own, so a new OUTER-owned file still lands — the inner
/// entries share the path prefix but attribute to the inner root. A
/// plain prefix count sees the map at the cap and wrongly refuses it.
#[tokio::test]
async fn nested_inner_entries_do_not_consume_outer_budget() {
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
    let first = open_published(&mut session, &use2_uri, 1, USE2).await;
    assert_eq!(
        count_code(&first, "RY010"),
        1,
        "the outer name must be unbound before c.R exists: {first}"
    );

    // The inner observer is created after the settle proof, so it
    // never enters the index; opening it observes the inner scope.
    fixture.write_file("pkg/obs.R", OBSERVER_INNER).unwrap();
    let obs_uri = file_uri(&fixture.path("pkg/obs.R"));
    let obs_first = open_published(&mut session, &obs_uri, 1, OBSERVER_INNER).await;
    assert_eq!(
        count_code(&obs_first, "RY010"),
        1,
        "the inner name must be unbound before b.R exists: {obs_first}"
    );

    // Inner-owned creation lands under the inner root's own budget
    // (one of four), which the inner observer proves.
    fixture.write_file("pkg/b.R", INNERDEFS).unwrap();
    let b_uri = file_uri(&fixture.path("pkg/b.R"));
    notify_created(&mut session, &b_uri).await;
    let inner_after = change_published(&mut session, &obs_uri, 2, OBSERVER_INNER).await;
    assert_eq!(
        count_code(&inner_after, "RY010"),
        0,
        "b.R is within the inner root's own budget and must land: {inner_after}"
    );

    // Outer-owned creation: the outer root's own subset is two of
    // four, so this must land even though the prefix count under
    // the outer root (two outer files plus inner keep.R and b.R)
    // sits at the cap.
    fixture.write_file("c.R", NEWDEFS).unwrap();
    let c_uri = file_uri(&fixture.path("c.R"));
    notify_created(&mut session, &c_uri).await;
    let after = change_published(&mut session, &use2_uri, 2, USE2).await;
    assert_eq!(
        count_code(&after, "RY010"),
        0,
        "c.R is within the outer root's own budget and must land: {after}"
    );

    join_session(session, server).await;
}
