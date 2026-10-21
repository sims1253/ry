//! A top-level name defined in several files resolves by path order, the
//! later file winning, as in `ry check`. An open buffer replaces the disk
//! bytes of its own path only: it does not outrank a closed file that
//! sorts after it, and opening or closing an unchanged file changes
//! nothing.
//!
//! `a.R` and `z.R` both define `f` and `use.R` calls `f()`. When `z.R`'s
//! integer `f` wins, `f() + 1L` is clean. When `a.R`'s character `f` wins,
//! `use.R` carries RY040.

use crate::harness;

use harness::{
    cli_diagnostics, drain_publications, file_uri, has_ry040, join_session, normalize_diagnostics,
    notify_changed, spawn_session, spawn_watching_session, sync_barrier,
};
use ry_testkit::FixtureProject;
use serde_json::json;

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

/// Opening and closing unchanged files in any order keeps `z.R`'s integer
/// `f` winning, and a cold session that opens the definitions in the other
/// order publishes the same diagnostics.
#[tokio::test]
async fn opening_and_closing_unchanged_documents_never_flips_use_r() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("z.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let z_uri = file_uri(&fixture.path("z.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;

    let only_use = harness::open_published(&mut session, &use_uri, 1, USE).await;
    assert!(!has_ry040(&only_use), "only use.R open: {only_use}");
    let mut states = Vec::new();
    for (step, uri, open) in [
        ("open a.R", &a_uri, Some((1, A_CHAR))),
        ("open z.R", &z_uri, Some((1, F_INT))),
        ("close a.R", &a_uri, None),
        ("reopen a.R", &a_uri, Some((2, A_CHAR))),
    ] {
        let mark = session.publication_mark();
        match open {
            Some((version, text)) => session.open(uri, version, text).await.unwrap(),
            None => session.close(uri).await.unwrap(),
        }
        let publish = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&publish),
            "{step} must keep use.R clean: {publish}"
        );
        states.push(publish);
    }
    join_session(session, server).await;

    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;
    let mark = session.publication_mark();
    session.open(&z_uri, 1, F_INT).await.unwrap();
    session.open(&a_uri, 1, A_CHAR).await.unwrap();
    session.open(&use_uri, 1, USE).await.unwrap();
    let reverse = session
        .published_diagnostics_after(&use_uri, mark)
        .await
        .unwrap();
    join_session(session, server).await;
    assert_eq!(
        normalize_diagnostics(&reverse),
        // The state after "open z.R": every file open.
        normalize_diagnostics(&states[1]),
        "didOpen order must not influence which definition wins"
    );
}

/// An unsaved edit replaces the disk bytes of its own path. `a.R` is the
/// only `f` and holds the character variant on disk. Editing the buffer to
/// the integer variant must clean `use.R`, like `ry check` on a tree whose
/// `a.R` holds the buffer bytes.
#[tokio::test]
async fn same_path_buffer_replaces_its_own_disk_bytes() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    // Neighboring valid control: a scratch tree with the integer
    // variant stays clean; the original tree must keep flagging so
    // the assertion cannot pass vacuously.
    let scratch = FixtureProject::empty().unwrap();
    scratch.write_file("a.R", F_INT).unwrap();
    scratch.write_file("use.R", USE).unwrap();
    let char_tree = cli_diagnostics(fixture.root(), fixture.root(), &[]);
    assert!(
        char_tree
            .iter()
            .any(|p| p.path.ends_with("use.R") && p.code == "RY040"),
        "fresh CLI on the char-f tree must flag use.R: {char_tree:?}"
    );
    let scratch_cli = cli_diagnostics(scratch.root(), scratch.root(), &[]);
    assert!(
        scratch_cli.is_empty(),
        "scratch CLI with the buffer bytes (int f) must be clean"
    );

    let a_uri = file_uri(&fixture.path("a.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;

    // The disk bytes resolve while a.R is closed: char f wins.
    let disk_state = harness::open_published(&mut session, &use_uri, 1, USE).await;
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
}

/// An open buffer gets no priority over a different closed file that sorts
/// after it. Both files define the integer `f` on disk and the open `a.R`
/// buffer holds the character variant: `z.R` still sorts last and wins.
#[tokio::test]
async fn open_buffer_does_not_outrank_a_later_sorted_closed_file() {
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
    let _ = harness::open_published(&mut session, &use_uri, 1, USE).await;
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
}

/// For every subset of {a.R, z.R, use.R} opened with their disk bytes,
/// `use.R` publishes exactly what `ry check` reports: nothing. With no file
/// open, nothing schedules a pass, so a watched event forces one.
#[tokio::test]
async fn open_closed_subsets_agree_with_the_fresh_project() {
    // The fresh-project oracle the subsets must converge to, run
    // explicitly instead of hardcoded: the integer `f` sorts last,
    // so the whole tree checks clean.
    {
        let oracle_fixture = FixtureProject::empty().unwrap();
        oracle_fixture.write_file("a.R", A_CHAR).unwrap();
        oracle_fixture.write_file("z.R", F_INT).unwrap();
        oracle_fixture.write_file("use.R", USE).unwrap();
        let oracle = cli_diagnostics(oracle_fixture.root(), oracle_fixture.root(), &[]);
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
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &use_uri).await;
    let mark = session.publication_mark();
    notify_changed(&mut session, &use_uri).await;
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
}
