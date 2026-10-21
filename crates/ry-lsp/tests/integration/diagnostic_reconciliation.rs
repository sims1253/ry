//! Every URI that stops being analyzed gets an empty publication, open or
//! closed. Each test keeps several documents in play and checks the
//! client's final per-URI store, where the last publication wins.

use crate::harness;

use harness::{file_uri, join_session, notify_changed, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Idle window for `quiesce_diagnostics`; comfortably exceeds the
/// server's 180ms debounce so a multi-URI burst is fully collected.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(500);

/// RY010 (unbound name) occurrences in one URI's stored publication.
fn ry010_count(diagnostics: &[Value]) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["code"] == json!("RY010"))
        .count()
}

/// The URI must be present in the final store with an empty set.
fn assert_cleared(store: &BTreeMap<String, Vec<Value>>, uri: &str) {
    assert!(
        store.get(uri).is_some_and(Vec::is_empty),
        "{uri} must end with an empty publication; final store: {store:?}"
    );
}

fn assert_ry010(store: &BTreeMap<String, Vec<Value>>, uri: &str) {
    assert!(
        store
            .get(uri)
            .is_some_and(|diagnostics| ry010_count(diagnostics) == 1),
        "{uri} must end with exactly one RY010; final store: {store:?}"
    );
}

/// Configuration and disk changes that drop documents from analysis clear
/// each of them, not only the last one scheduled. `a.R`, `b.R`, and `c.R`
/// are open. `d.R` and `e.R` stay closed and publish through the index.
#[tokio::test]
async fn config_transitions_clear_every_dropped_document() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", "\n").unwrap();
    let names = ["a", "b", "c", "d", "e"];
    let uris: Vec<String> = names
        .iter()
        .map(|name| {
            let path = fixture
                .write_file(
                    format!("{name}.R"),
                    format!("{name}_value <- never_bound_here\n"),
                )
                .unwrap();
            file_uri(&path)
        })
        .collect();
    let [a, b, c, d, e] = [0, 1, 2, 3, 4].map(|index| uris[index].as_str());
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, a).await;

    let mark = session.publication_mark();
    for (uri, name) in [(a, "a"), (b, "b"), (c, "c")] {
        session
            .open(uri, 1, &format!("{name}_value <- never_bound_here\n"))
            .await
            .unwrap();
    }
    let initial = session.quiesce_diagnostics(a, mark, DRAIN).await.unwrap();
    for uri in [a, b, c, d, e] {
        assert_ry010(&initial, uri);
    }

    // Disabling the folder clears every open document, and re-enabling
    // restores them, so the clear was a publication.
    for (enable, expect_cleared) in [(false, true), (true, false)] {
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": { "ry": { "enable": enable } } }),
            )
            .await
            .unwrap();
        let store = session.quiesce_diagnostics(a, mark, DRAIN).await.unwrap();
        for uri in [a, b, c] {
            if expect_cleared {
                assert_cleared(&store, uri);
            } else {
                assert_ry010(&store, uri);
            }
        }
    }

    // A rescan drops the deleted closed `e.R`. A plain `.R` deletion does
    // not trigger one, so the event is an `ry.toml` change.
    std::fs::remove_file(fixture.path("e.R")).unwrap();
    let mark = session.publication_mark();
    notify_changed(&mut session, &file_uri(&fixture.path("ry.toml"))).await;
    let deleted = session.quiesce_diagnostics(b, mark, DRAIN).await.unwrap();
    assert_cleared(&deleted, e);
    assert_ry010(&deleted, b);

    // Excluding two of the three open documents clears both while `b.R`
    // republishes. The closed `d.R` is excluded too.
    fixture
        .write_file("ry.toml", "exclude = [\"a.R\", \"c.R\", \"d.R\"]\n")
        .unwrap();
    let mark = session.publication_mark();
    session
        .notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": {} }),
        )
        .await
        .unwrap();
    let excluded = session.quiesce_diagnostics(b, mark, DRAIN).await.unwrap();
    for uri in [a, c, d] {
        assert_cleared(&excluded, uri);
    }
    assert_ry010(&excluded, b);

    join_session(session, server).await;
}

/// Removing a workspace folder clears the closed files published under it.
/// `keep.R` in the surviving folder stays open and drives the republish.
#[tokio::test]
async fn removing_a_folder_clears_its_closed_disk_files() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("A/keep.R", "w <- never_bound_here\n")
        .unwrap();
    fixture
        .write_file("B/a.R", "x <- never_bound_here\n")
        .unwrap();
    let folder_a = fixture.path("A");
    let folder_b = fixture.path("B");
    let folder_b_uri = file_uri(&folder_b);
    let a_uri = file_uri(&fixture.path("B/a.R"));
    let keep_uri = file_uri(&fixture.path("A/keep.R"));
    let (mut session, server) = spawn_session(&[&folder_a, &folder_b], json!({}), None).await;

    sync_barrier(&mut session, &keep_uri).await;
    let mark = session.publication_mark();
    session
        .open(&keep_uri, 1, "w <- never_bound_here\n")
        .await
        .unwrap();
    let initial = session
        .quiesce_diagnostics(&keep_uri, mark, DRAIN)
        .await
        .unwrap();
    assert_ry010(&initial, &a_uri);
    assert_ry010(&initial, &keep_uri);

    let mark = session.publication_mark();
    session
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({
                "event": {
                    "added": [],
                    "removed": [{"uri": folder_b_uri, "name": "B"}]
                }
            }),
        )
        .await
        .unwrap();
    let removed = session
        .quiesce_diagnostics(&keep_uri, mark, DRAIN)
        .await
        .unwrap();
    assert_cleared(&removed, &a_uri);
    assert_ry010(&removed, &keep_uri);

    join_session(session, server).await;
}

/// Removing the last workspace folder stops analysis of its closed files.
/// The client's `rootUri` named the removed folder, and neither a later
/// configuration change (a rescan) nor a watched-file event may index the
/// removed directory again through that root.
#[tokio::test]
async fn removing_every_folder_never_reindexes_the_removed_root() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("open.R", "w <- never_bound_here\n")
        .unwrap();
    fixture
        .write_file("closed.R", "x <- never_bound_here\n")
        .unwrap();
    let root_uri = file_uri(fixture.root());
    let open_uri = file_uri(&fixture.path("open.R"));
    let closed_uri = file_uri(&fixture.path("closed.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;

    sync_barrier(&mut session, &open_uri).await;
    let mark = session.publication_mark();
    session
        .open(&open_uri, 1, "w <- never_bound_here\n")
        .await
        .unwrap();
    let initial = session
        .quiesce_diagnostics(&open_uri, mark, DRAIN)
        .await
        .unwrap();
    assert_ry010(&initial, &closed_uri);

    let mark = session.publication_mark();
    session
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({"event": {"added": [], "removed": [{"uri": root_uri, "name": "root"}]}}),
        )
        .await
        .unwrap();
    let removed = session
        .quiesce_diagnostics(&closed_uri, mark, DRAIN)
        .await
        .unwrap();
    assert_cleared(&removed, &closed_uri);

    // The still-open buffer is analyzed as a loose file; its publication
    // marks the end of each pass.
    for (method, params) in [
        (
            "workspace/didChangeConfiguration",
            json!({ "settings": {} }),
        ),
        (
            "workspace/didChangeWatchedFiles",
            json!({ "changes": [{ "uri": closed_uri, "type": 2 }] }),
        ),
    ] {
        let mark = session.publication_mark();
        session.notify(method, params).await.unwrap();
        let store = session
            .quiesce_diagnostics(&open_uri, mark, DRAIN)
            .await
            .unwrap();
        assert!(
            store.get(&closed_uri).is_none_or(Vec::is_empty),
            "{method} re-indexed the removed folder: {store:?}"
        );
    }

    join_session(session, server).await;
}
