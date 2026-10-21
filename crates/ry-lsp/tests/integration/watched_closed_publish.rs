//! With no document open, a watched event republishes the closed files it
//! touches, checked against current resolution context: configured
//! globals, `load()` bindings, and report groups.

use crate::harness;

use harness::{
    file_uri, join_session, normalize_diagnostics, notify_changed, notify_created, open_published,
    spawn_session, spawn_watching_session, sync_barrier,
};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

fn has_ry010_for(publish: &Value, name: &str) -> bool {
    normalize_diagnostics(publish).iter().any(|diagnostic| {
        diagnostic["code"] == json!("RY010")
            && diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains(name))
    })
}

#[tokio::test]
async fn watched_events_with_no_open_documents_republish_closed_files() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file(
            "ry.toml",
            "globals = [\"my_global\"]\n[reports]\nenabled = true\n",
        )
        .unwrap();
    fixture.write_file("main.R", "w <- 1L\n").unwrap();
    fixture
        .write_file("util.R", "x <- never_bound_here\n")
        .unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    let util_uri = file_uri(&fixture.path("util.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &main_uri).await;

    // Opening clean `main.R` runs a pass that also publishes RY010 for the
    // never-opened `util.R`. Publication waits for the initial index.
    let mark = session.publication_mark();
    let main_publish = open_published(&mut session, &main_uri, 1, "w <- 1L\n").await;
    assert!(
        normalize_diagnostics(&main_publish).is_empty(),
        "main.R must be clean: {main_publish}"
    );
    let stale = session
        .published_diagnostics_after(&util_uri, mark)
        .await
        .unwrap();
    assert!(
        has_ry010_for(&stale, "never_bound_here"),
        "the pass must publish RY010 for closed util.R: {stale}"
    );
    // The closed `util.R` keeps its diagnostics.
    session.close(&main_uri).await.unwrap();
    sync_barrier(&mut session, &main_uri).await;

    std::fs::write(fixture.path("util.R"), "x <- 1L\n").unwrap();
    let mark = session.publication_mark();
    notify_changed(&mut session, &util_uri).await;
    let fixed = session
        .published_diagnostics_after(&util_uri, mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&fixed).is_empty(),
        "the watched fix must clear util.R's RY010: {fixed}"
    );

    // A created file resolves the configured global without a rescan.
    fixture.write_file("new.R", "y <- my_global\n").unwrap();
    let new_uri = file_uri(&fixture.path("new.R"));
    let mark = session.publication_mark();
    notify_created(&mut session, &new_uri).await;
    let created = session
        .published_diagnostics_after(&new_uri, mark)
        .await
        .unwrap();
    assert!(
        !has_ry010_for(&created, "my_global"),
        "the created file must resolve the configured global: {created}"
    );

    // `ordinary.rda` binds `a` (checked in R). Only the use of `a` before
    // the `load()` line is unbound, and moving `load()` above both uses
    // must move its bindings too.
    std::fs::write(
        fixture.path("data.rda"),
        include_bytes!("../../../ry-testkit/testdata/complete-package/data/ordinary.rda"),
    )
    .unwrap();
    fixture
        .write_file("loader.R", "a\nload(\"data.rda\")\na\n")
        .unwrap();
    let loader_uri = file_uri(&fixture.path("loader.R"));
    let mark = session.publication_mark();
    notify_created(&mut session, &loader_uri).await;
    let created = session
        .published_diagnostics_after(&loader_uri, mark)
        .await
        .unwrap();
    let ry010_lines: Vec<_> = normalize_diagnostics(&created)
        .iter()
        .filter(|diagnostic| diagnostic["code"] == json!("RY010"))
        .map(|diagnostic| diagnostic["range"]["start"]["line"].clone())
        .collect();
    assert_eq!(ry010_lines, [json!(0)], "{created}");
    assert!(has_ry010_for(&created, "`a`"), "{created}");

    std::fs::write(fixture.path("loader.R"), "load(\"data.rda\")\na\na\n").unwrap();
    let mark = session.publication_mark();
    notify_changed(&mut session, &loader_uri).await;
    let moved = session
        .published_diagnostics_after(&loader_uri, mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&moved)
            .iter()
            .all(|diagnostic| diagnostic["code"] != json!("RY010")),
        "moving load() above the uses must clear the stale span entry: {moved}"
    );

    // A report is its own resolution group, which must contain the report.
    fixture
        .write_file("memo.qmd", "```{r}\nz <- my_global\n```\n")
        .unwrap();
    let memo_uri = file_uri(&fixture.path("memo.qmd"));
    let mark = session.publication_mark();
    notify_created(&mut session, &memo_uri).await;
    let report = session
        .published_diagnostics_after(&memo_uri, mark)
        .await
        .unwrap();
    assert!(
        !has_ry010_for(&report, "my_global"),
        "the created report must resolve the configured global: {report}"
    );

    join_session(session, server).await;
}

/// Closing the last open document before its debounced pass ran ends with
/// the close's empty publication: the pending pass must not republish the
/// file's disk diagnostics afterwards.
#[tokio::test]
async fn closing_the_last_document_before_its_pass_publishes_nothing_more() {
    let fixture = FixtureProject::empty().unwrap();
    let source = "x <- never_bound_here\n";
    fixture.write_file("util.R", source).unwrap();
    let util_uri = file_uri(&fixture.path("util.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &util_uri).await;

    let mark = session.publication_mark();
    session.open(&util_uri, 1, source).await.unwrap();
    session.close(&util_uri).await.unwrap();
    // A pass may legitimately publish while the buffer is open; the
    // close's clear is the last word.
    let mut mark = mark;
    loop {
        let publish = session
            .published_diagnostics_after(&util_uri, mark)
            .await
            .unwrap();
        if normalize_diagnostics(&publish).is_empty() {
            break;
        }
        mark = session.publication_mark();
    }
    let after = session.publication_mark();
    let leaked = tokio::time::timeout(
        std::time::Duration::from_millis(700),
        session.published_diagnostics_after(&util_uri, after),
    )
    .await;
    assert!(
        leaked.is_err(),
        "no publication may follow the close's clear; got {:?}",
        leaked.ok()
    );

    join_session(session, server).await;
}
