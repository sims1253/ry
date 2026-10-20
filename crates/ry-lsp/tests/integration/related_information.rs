//! RY090/RY091 related locations reach `relatedInformation` only for
//! clients that advertise it. The range is UTF-16: `𝑥` is two units, so
//! the formal the CLI reports at 1-based scalar column 31 starts at
//! character 31 here.

use crate::harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

const SOURCE: &str = "# Größe
helper <- function(lbl = \"𝑥\", scale) nchar(lbl) * scale
use <- function() {
  é <- \"a\"; helper(é)
}
";

/// The `relatedInformation` of each published diagnostic.
fn related(published: &Value) -> Vec<Value> {
    normalize_diagnostics(published)
        .iter()
        .map(|d| d.get("relatedInformation").cloned().unwrap_or(Value::Null))
        .collect()
}

#[tokio::test]
async fn related_information_follows_the_capability_and_edits() {
    let fixture = FixtureProject::empty().unwrap();
    let path = fixture.write_file("uni.R", SOURCE).unwrap();
    let uri = file_uri(&path);
    let declared = |line: u32| {
        json!([{
            "location": {
                "uri": uri,
                "range": {"start": {"line": line, "character": 31}, "end": {"line": line, "character": 36}},
            },
            "message": "`scale` is declared here without a default value",
        }])
    };

    let capabilities =
        json!({"textDocument": {"publishDiagnostics": {"relatedInformation": true}}});
    let (mut session, server) = spawn_session(&[fixture.root()], capabilities, None).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, SOURCE).await.unwrap();
    let published = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert_eq!(related(&published), [declared(1)], "{published}");

    let mark = session.publication_mark();
    let moved = format!("\n{SOURCE}");
    session
        .change(&uri, 2, json!([{"text": moved}]))
        .await
        .unwrap();
    let published = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert_eq!(related(&published), [declared(2)], "{published}");
    join_session(session, server).await;

    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, SOURCE).await.unwrap();
    let published = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert_eq!(related(&published), [Value::Null], "{published}");
    join_session(session, server).await;
}
