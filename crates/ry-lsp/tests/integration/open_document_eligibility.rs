//! Open documents pass the same `index.max-file-bytes` and `index.max-depth`
//! gates as the background index. The size gate measures the buffer, not
//! the file on disk. The depth gate counts the containing directory's
//! components below the folder root.
//!
//! `def.R` defines the character `f` and `use.R` calls `f() + 1L`, so RY040
//! in `use.R` shows that `def.R` takes part in the analysis.

use crate::harness;

use harness::{file_uri, has_ry040, join_session, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Idle window for `quiesce_diagnostics`; comfortably exceeds the
/// server's 180ms debounce so a multi-URI burst is fully collected.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(500);

/// 36 bytes: over the 32-byte cap, under the 64-byte one.
const DEF_CHAR: &str = "f <- function(argument) \"str_value\"\n";
const USE: &str = "x <- f() + 1L\n";

fn last_publication<'a>(store: &'a BTreeMap<String, Vec<Value>>, uri: &str) -> &'a [Value] {
    store.get(uri).map(Vec::as_slice).unwrap_or(&[])
}

fn ry040_in(store: &BTreeMap<String, Vec<Value>>, uri: &str) -> bool {
    last_publication(store, uri)
        .iter()
        .any(|diagnostic| diagnostic["code"] == json!("RY040"))
}

/// Opening a file the closed index omits does not admit it either. A file
/// whose parent sits exactly at `max-depth` is admitted.
#[tokio::test]
async fn open_buffers_pass_the_index_gates() {
    // (index config, definition path, whether its `f` takes part)
    for (config, def_path, eligible) in [
        ("max-file-bytes = 32", "def.R", false),
        ("max-depth = 1", "a/b/def.R", false),
        ("max-depth = 1", "a/def.R", true),
    ] {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", format!("[index]\n{config}\n"))
            .unwrap();
        fixture.write_file(def_path, DEF_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let def_uri = file_uri(&fixture.path(def_path));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
        let publish = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            has_ry040(&publish),
            eligible,
            "{config}, {def_path}: {publish}"
        );
        // Every open document is acknowledged with a publication. An
        // ineligible one stays out of project state with no diagnostics
        // from it.
        let def_publish = session
            .published_diagnostics_after(&def_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&def_publish),
            "{config}, {def_path}: {def_publish}"
        );
        join_session(session, server).await;
    }
}

/// Edits and config reloads move an open buffer across the size cap in
/// both directions. An ejected buffer ends with an empty publication, and
/// `use.R` loses the RY040 that the buffer's `f` caused.
#[tokio::test]
async fn crossing_the_size_cap_ejects_and_readmits_an_open_buffer() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "[index]\nmax-file-bytes = 64\n")
        .unwrap();
    fixture.write_file("def.R", DEF_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let def_uri = file_uri(&fixture.path("def.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;

    let mark = session.publication_mark();
    session.open(&use_uri, 1, USE).await.unwrap();
    session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
    let initial = session
        .quiesce_diagnostics(&use_uri, mark, DRAIN)
        .await
        .unwrap();
    assert!(ry040_in(&initial, &use_uri), "under the cap: {initial:?}");

    // A trailing comment grows the buffer past the cap and keeps one
    // binding. The file on disk does not change.
    let oversized = format!("{DEF_CHAR}# {}\n", "x".repeat(64));
    assert!(oversized.len() > 64);
    let steps = [
        ("edit past the cap", Some(oversized.as_str()), None, false),
        ("edit back under the cap", Some(DEF_CHAR), None, true),
        ("lower the cap", None, Some(32), false),
        ("raise the cap", None, Some(4096), true),
    ];
    for (version, (step, text, cap, eligible)) in (2..).zip(steps) {
        let mark = session.publication_mark();
        if let Some(text) = text {
            session
                .change(&def_uri, version, json!([{"text": text}]))
                .await
                .unwrap();
        }
        if let Some(cap) = cap {
            fixture
                .write_file("ry.toml", format!("[index]\nmax-file-bytes = {cap}\n"))
                .unwrap();
            session
                .notify("workspace/didChangeConfiguration", json!({"settings": {}}))
                .await
                .unwrap();
        }
        let store = session
            .quiesce_diagnostics(&use_uri, mark, DRAIN)
            .await
            .unwrap();
        assert_eq!(ry040_in(&store, &use_uri), eligible, "{step}: {store:?}");
        if !eligible {
            assert!(
                last_publication(&store, &def_uri).is_empty(),
                "{step}: the ejected buffer must end with an empty publication: {store:?}"
            );
        }
    }

    join_session(session, server).await;
}

/// A buffer opened over the cap was never admitted. Raising the cap admits
/// it on the reload, without reopening it.
#[tokio::test]
async fn raising_the_cap_admits_a_buffer_opened_over_it() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "[index]\nmax-file-bytes = 32\n")
        .unwrap();
    fixture.write_file("def.R", DEF_CHAR).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let def_uri = file_uri(&fixture.path("def.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;

    let mark = session.publication_mark();
    session.open(&use_uri, 1, USE).await.unwrap();
    session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
    let excluded = session
        .quiesce_diagnostics(&use_uri, mark, DRAIN)
        .await
        .unwrap();
    assert!(!ry040_in(&excluded, &use_uri), "over the cap: {excluded:?}");

    fixture
        .write_file("ry.toml", "[index]\nmax-file-bytes = 4096\n")
        .unwrap();
    let mark = session.publication_mark();
    session
        .notify("workspace/didChangeConfiguration", json!({"settings": {}}))
        .await
        .unwrap();
    let readmitted = session
        .published_diagnostics_after(&use_uri, mark)
        .await
        .unwrap();
    assert!(
        has_ry040(&readmitted),
        "after raising the cap: {readmitted}"
    );
    join_session(session, server).await;
}

/// Ineligible open documents keep lightweight editor features: inlay
/// hints are computed from the single document's own parse and never
/// enter project-wide state, so they still serve a size-gated buffer
/// (while `enable: false` keeps returning null — the editor
/// intentionally disabled ry there).
#[tokio::test]
async fn ineligible_open_buffer_keeps_inlay_hints() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "[index]\nmax-file-bytes = 32\n")
        .unwrap();
    let source = format!("my_value <- 1L\n# {}\n", "x".repeat(32));
    assert!(source.len() > 32);
    fixture.write_file("big.R", &source).unwrap();
    let uri = file_uri(&fixture.path("big.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &uri).await;

    let publish = harness::open_published(&mut session, &uri, 1, &source).await;
    assert!(
        publish["params"]["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "the over-cap buffer carries no project diagnostics: {publish}"
    );
    let hints = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 0}},
            }),
        )
        .await
        .unwrap();
    assert!(
        hints
            .as_array()
            .is_some_and(|hints| hints.iter().any(|hint| {
                hint["position"] == json!({"line": 0, "character": 8})
                    && hint["label"] == json!(": integer<len=1>")
            })),
        "single-document hints must still serve the ineligible buffer: {hints}"
    );

    join_session(session, server).await;
}
