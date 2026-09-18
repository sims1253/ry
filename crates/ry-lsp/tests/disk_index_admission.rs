//! #524: watched-file admission must agree with full discovery for every
//! pattern class.
//!
//! Full discovery prunes exclusion at the directory entry — a bare
//! `exclude = ["vendor"]` stops the walk from ever entering `vendor/` —
//! and applies `.Rbuildignore` (with `include_build_ignored` rescue) to
//! every entry. The per-file admission path used by watched-file events
//! used to match `Excludes::matches` against the *file's* relative path
//! only and never consulted `.Rbuildignore` at all, so a watched
//! create/change event for `vendor/defs.R` landed a file the walk would
//! never admit, and the editor disagreed with `ry check` for the rest of
//! the session.
//!
//! These tests pin the converged contract through the issue's verbatim
//! repro: `main.R` reads the unbound name `foo` (one RY010), and a watched
//! file appearing outside the editor defines `foo`. When the new file is
//! walk-inadmissible the RY010 must survive and the session must converge
//! with a fresh server over the same tree; when an explicit
//! `include_build_ignored` override rescues it, the RY010 must clear like
//! a fresh server's does.
//!
//! The first three tests fail against the old per-file check (the excluded
//! file lands and the RY010 disappears); the override test passes both
//! ways and guards against over-correction.

mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

/// `main.R` reads the unbound name `foo`: exactly one RY010 while no
/// indexed file defines it.
const MAIN: &str = "x <- foo\n";
/// A definition of `foo` that, once indexed, resolves the read in
/// `main.R` and clears its RY010.
const DEFS: &str = "foo <- function() 1\n";

fn count_ry010(publish: &Value) -> usize {
    normalize_diagnostics(publish)
        .iter()
        .filter(|diagnostic| diagnostic["code"] == json!("RY010"))
        .count()
}

/// Open `main.R` (whose content never changes) and return the fresh
/// publication for it after `mark`.
async fn open_main_and_observe(
    session: &mut harness::ClientSession,
    main_uri: &str,
    mark: u64,
) -> Value {
    session.open(main_uri, 1, MAIN).await.unwrap();
    session
        .published_diagnostics_after(main_uri, mark)
        .await
        .unwrap()
}

/// A fresh server over the same final filesystem, opening only `main.R`:
/// the oracle every watched transition must converge with.
async fn fresh_main_diagnostics(fixture: &FixtureProject) -> Value {
    let main_uri = file_uri(&fixture.path("main.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &main_uri).await;
    let mark = session.publication_mark();
    let publish = open_main_and_observe(&mut session, &main_uri, mark).await;
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

/// Deliver a watched creation event for `uri`, the way the client reports
/// a file generated outside the editor.
async fn notify_created(session: &mut harness::ClientSession, uri: &str) {
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": uri, "type": 1}]}),
        )
        .await
        .unwrap();
}

/// A watched creation under a directory pruned by a bare `exclude`
/// pattern must not enter the index: the RY010 on `main.R` survives.
#[test]
fn vendor_excluded_directory_stays_out_of_the_watched_index() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "exclude = [\"vendor\"]\n")
            .unwrap();
        fixture.write_file("main.R", MAIN).unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &main_uri).await;

        // `foo` is unbound: the initial index holds only `main.R`.
        let mark = session.publication_mark();
        let first = open_main_and_observe(&mut session, &main_uri, mark).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "foo must be unbound before vendor/ exists: {first}"
        );

        // Generate `vendor/defs.R` outside the editor and deliver the
        // watched creation the client reports for it.
        fixture.write_file("vendor/defs.R", DEFS).unwrap();
        let defs_uri = file_uri(&fixture.path("vendor/defs.R"));
        let event_mark = session.publication_mark();
        notify_created(&mut session, &defs_uri).await;
        let after = session
            .published_diagnostics_after(&main_uri, event_mark)
            .await
            .unwrap();
        assert_eq!(
            count_ry010(&after),
            1,
            "vendor/defs.R is pruned by the walk and must stay out of the index: {after}"
        );
        assert_converges(
            &after,
            &fresh_main_diagnostics(&fixture).await,
            "vendor exclusion",
        );

        join_session(session, server).await;
    });
}

/// The ancestor check must climb past the parent: a watched file nested
/// two levels under the excluded directory stays out too.
#[test]
fn vendor_nested_grandchild_stays_out_of_the_watched_index() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "exclude = [\"vendor\"]\n")
            .unwrap();
        fixture.write_file("main.R", MAIN).unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &main_uri).await;

        let mark = session.publication_mark();
        let first = open_main_and_observe(&mut session, &main_uri, mark).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "foo must be unbound before vendor/ exists: {first}"
        );

        fixture.write_file("vendor/nested/deep.R", DEFS).unwrap();
        let deep_uri = file_uri(&fixture.path("vendor/nested/deep.R"));
        let event_mark = session.publication_mark();
        notify_created(&mut session, &deep_uri).await;
        let after = session
            .published_diagnostics_after(&main_uri, event_mark)
            .await
            .unwrap();
        assert_eq!(
            count_ry010(&after),
            1,
            "vendor/nested/deep.R is pruned by the walk and must stay out of the index: {after}"
        );
        assert_converges(
            &after,
            &fresh_main_diagnostics(&fixture).await,
            "nested vendor exclusion",
        );

        join_session(session, server).await;
    });
}

/// A watched creation under a `.Rbuildignore`-pruned directory must not
/// enter the index either: the walk never descends into `vignettes/`
/// here, so neither may the per-file path.
#[test]
fn buildignored_directory_stays_out_of_the_watched_index() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("DESCRIPTION", "Package: example\n")
            .unwrap();
        fixture
            .write_file(".Rbuildignore", "^vignettes$\n")
            .unwrap();
        fixture.write_file("main.R", MAIN).unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &main_uri).await;

        let mark = session.publication_mark();
        let first = open_main_and_observe(&mut session, &main_uri, mark).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "foo must be unbound before vignettes/ exists: {first}"
        );

        fixture.write_file("vignettes/drop.R", DEFS).unwrap();
        let drop_uri = file_uri(&fixture.path("vignettes/drop.R"));
        let event_mark = session.publication_mark();
        notify_created(&mut session, &drop_uri).await;
        let after = session
            .published_diagnostics_after(&main_uri, event_mark)
            .await
            .unwrap();
        assert_eq!(
            count_ry010(&after),
            1,
            "vignettes/drop.R is build-ignored by the walk and must stay out of the index: {after}"
        );
        assert_converges(
            &after,
            &fresh_main_diagnostics(&fixture).await,
            "buildignore exclusion",
        );

        join_session(session, server).await;
    });
}

/// The explicit override keeps working through the shared policy: a
/// watched file rescued by `include_build_ignored` is still admitted and
/// clears the RY010, matching a fresh server. This passes against the old
/// per-file check too — it guards the new verdict against
/// over-correction rather than discriminating the fix.
#[test]
fn include_build_ignored_override_still_admits_watched_files() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("DESCRIPTION", "Package: example\n")
            .unwrap();
        fixture
            .write_file(".Rbuildignore", "^vignettes$\n")
            .unwrap();
        fixture
            .write_file(
                "ry.toml",
                "include-build-ignored = [\"vignettes/keep.R\"]\n",
            )
            .unwrap();
        fixture.write_file("main.R", MAIN).unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &main_uri).await;

        let mark = session.publication_mark();
        let first = open_main_and_observe(&mut session, &main_uri, mark).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "foo must be unbound before vignettes/ exists: {first}"
        );

        fixture.write_file("vignettes/keep.R", DEFS).unwrap();
        let keep_uri = file_uri(&fixture.path("vignettes/keep.R"));
        let event_mark = session.publication_mark();
        notify_created(&mut session, &keep_uri).await;
        let after = session
            .published_diagnostics_after(&main_uri, event_mark)
            .await
            .unwrap();
        assert_eq!(
            count_ry010(&after),
            0,
            "vignettes/keep.R is rescued by include_build_ignored and must enter the index: {after}"
        );
        assert_converges(
            &after,
            &fresh_main_diagnostics(&fixture).await,
            "include_build_ignored override",
        );

        join_session(session, server).await;
    });
}

/// A watched event addressed through a symlinked directory must not
/// enter the index: the walk never descends through symlinks, so the
/// per-file verdict refuses the through-link spelling exactly like the
/// walk skips it. There is deliberately no fresh-server oracle here:
/// the fresh walk indexes the same bytes under their real spelling, so
/// whole-tree convergence needs a rescan; the live invariant — the
/// RY010 survives because the through-link spelling never lands — is
/// what the refusal guarantees.
#[cfg(unix)]
#[test]
fn through_symlink_directory_stays_out_of_the_watched_index() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("main.R", MAIN).unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &main_uri).await;

        let mark = session.publication_mark();
        let first = open_main_and_observe(&mut session, &main_uri, mark).await;
        assert_eq!(
            count_ry010(&first),
            1,
            "foo must be unbound before link/ exists: {first}"
        );

        // The bytes exist under their real spelling; the client watches
        // the through-link spelling (built without the canonicalizing
        // `file_uri`, which would resolve the link away).
        fixture.write_file("real/defs.R", DEFS).unwrap();
        std::os::unix::fs::symlink(fixture.path("real"), fixture.path("link")).unwrap();
        let link_uri = format!("{}/link/defs.R", file_uri(fixture.root()).as_str());
        let event_mark = session.publication_mark();
        notify_created(&mut session, &link_uri).await;
        let after = session
            .published_diagnostics_after(&main_uri, event_mark)
            .await
            .unwrap();
        assert_eq!(
            count_ry010(&after),
            1,
            "link/defs.R is skipped by the walk and must stay out of the index: {after}"
        );

        join_session(session, server).await;
    });
}
