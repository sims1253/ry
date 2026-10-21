//! A watched file enters the index only if full discovery would admit it.
//! `main.R` reads the unbound `foo` (one RY010), and a file created outside
//! the editor defines `foo`: RY010 stays exactly when the walk would skip
//! that file.

use crate::harness;

use harness::{
    assert_converges, count_code, file_uri, join_session, notify_created, spawn_session,
    sync_barrier,
};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

/// `main.R` reads the unbound name `foo`: exactly one RY010 while no
/// indexed file defines it.
const MAIN: &str = "x <- foo\n";
/// A definition of `foo` that, once indexed, resolves the read in
/// `main.R` and clears its RY010.
const DEFS: &str = "foo <- function() 1\n";

/// A session on `fixture` with `main.R` open after the initial index, and
/// its first publication.
async fn open_main(fixture: &FixtureProject) -> (harness::ClientSession, harness::Server, Value) {
    let main_uri = file_uri(&fixture.path("main.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &main_uri).await;
    let publish = harness::open_published(&mut session, &main_uri, 1, MAIN).await;
    (session, server, publish)
}

#[tokio::test]
async fn watched_creation_follows_discovery_admission() {
    let package = [
        ("DESCRIPTION", "Package: example\n"),
        (".Rbuildignore", "^vignettes$\n"),
    ];
    let rescue = (
        "ry.toml",
        "include-build-ignored = [\"vignettes/keep.R\"]\n",
    );
    let exclude = [("ry.toml", "exclude = [\"vendor\"]\n")];
    // (setup files, created file, RY010 count after the event)
    let cases = [
        // A bare `exclude` prunes the directory.
        (&exclude[..], "vendor/defs.R", 1),
        // The ancestor check climbs past the parent.
        (&exclude, "vendor/nested/deep.R", 1),
        // `.Rbuildignore` prunes the directory.
        (&package[..], "vignettes/drop.R", 1),
        // `include-build-ignored` still admits the file.
        (&[package[0], package[1], rescue], "vignettes/keep.R", 0),
    ];
    for (setup, created, expected) in cases {
        let fixture = FixtureProject::empty().unwrap();
        for (path, text) in setup {
            fixture.write_file(path, text).unwrap();
        }
        fixture.write_file("main.R", MAIN).unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server, first) = open_main(&fixture).await;
        assert_eq!(
            count_code(&first, "RY010"),
            1,
            "{created}: foo must be unbound before the file exists: {first}"
        );

        fixture.write_file(created, DEFS).unwrap();
        let event_mark = session.publication_mark();
        notify_created(&mut session, &file_uri(&fixture.path(created))).await;
        let after = session
            .published_diagnostics_after(&main_uri, event_mark)
            .await
            .unwrap();
        assert_eq!(count_code(&after, "RY010"), expected, "{created}: {after}");
        let (fresh, fresh_server, fresh_publish) = open_main(&fixture).await;
        join_session(fresh, fresh_server).await;
        assert_converges(&after, &fresh_publish, created);
        join_session(session, server).await;
    }
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
#[tokio::test]
async fn through_symlink_directory_stays_out_of_the_watched_index() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", MAIN).unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    let (mut session, server, first) = open_main(&fixture).await;
    assert_eq!(
        count_code(&first, "RY010"),
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
        count_code(&after, "RY010"),
        1,
        "link/defs.R is skipped by the walk and must stay out of the index: {after}"
    );

    join_session(session, server).await;
}
