//! Inline suppression holds for a file the editor never opened and for a
//! file after it is closed, as it does while the file is open.
//!
//! `a.R` has two RY040 triggers: line 1 carries `# ry: ignore[RY040]`, line
//! 3 does not, so exactly the LSP line-2 finding survives. The server
//! publishes closed files only from a pass that an open document triggers,
//! so `other.R` stays open throughout.

use crate::harness;

use harness::{file_uri, join_session, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

const SUPPRESSED_SRC: &str = "x <- \"a\" * 3  # ry: ignore[RY040]\ny <- 2L\nz <- \"b\" * 4\n";
const OTHER_SRC: &str = "w <- 1L\n";

/// Sorted LSP start lines of the RY040 diagnostics.
fn ry040_lines(diagnostics: &[Value]) -> Vec<u64> {
    let mut lines: Vec<u64> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["code"] == json!("RY040"))
        .filter_map(|diagnostic| {
            diagnostic
                .pointer("/range/start/line")
                .and_then(Value::as_u64)
        })
        .collect();
    lines.sort_unstable();
    lines
}

fn published_ry040_lines(publish: &Value) -> Vec<u64> {
    ry040_lines(
        publish["params"]["diagnostics"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default(),
    )
}

#[tokio::test]
async fn inline_suppression_holds_before_open_while_open_and_after_close() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", SUPPRESSED_SRC).unwrap();
    fixture.write_file("other.R", OTHER_SRC).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let other_uri = file_uri(&fixture.path("other.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &other_uri).await;

    // Never opened: a.R arrives through the background index.
    let mark = session.publication_mark();
    session.open(&other_uri, 1, OTHER_SRC).await.unwrap();
    let publish = session
        .published_diagnostics_after(&a_uri, mark)
        .await
        .unwrap();
    assert_eq!(
        published_ry040_lines(&publish),
        vec![2],
        "never-opened a.R: {publish}"
    );

    sync_barrier(&mut session, &other_uri).await;
    let open_publish = harness::open_published(&mut session, &a_uri, 1, SUPPRESSED_SRC).await;
    assert_eq!(
        published_ry040_lines(&open_publish),
        vec![2],
        "open a.R: {open_publish}"
    );

    // didClose clears a.R, then the pass for the still-open other.R
    // publishes it again from the disk index. The last publication per URI
    // wins in the quiesce map. The idle window exceeds the 180ms debounce
    // so the republish is collected.
    let close_mark = session.publication_mark();
    session.close(&a_uri).await.unwrap();
    let burst = session
        .quiesce_diagnostics(&a_uri, close_mark, std::time::Duration::from_millis(500))
        .await
        .unwrap();
    let closed = burst
        .get(&a_uri)
        .expect("the republish must cover the closed a.R again");
    assert_eq!(ry040_lines(closed), vec![2], "closed a.R: {closed:?}");

    join_session(session, server).await;
}
