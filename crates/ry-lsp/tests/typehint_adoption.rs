//! Adopted source annotations use the same warm checking path in the editor.

mod harness;

use harness::{join_session, spawn_session, sync_barrier};
use ry_testkit::{FixtureProject, file_uri};
use serde_json::{Value, json};

fn count_code(publish: &Value, code: &str) -> usize {
    publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["code"] == code)
        .count()
}

#[test]
fn annotation_only_editor_edits_retract_and_restore_mismatch() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file(
                    "ry.toml",
                    "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
                )
                .unwrap();
            let source = |annotation: &str| {
                format!(
                    "label <- \"\u{1f600}\"\nf <- function(x) {{\n  {annotation}\n  x\n}}\nf(\"bad\")\n"
                )
            };
            let first = source("#| x integer");
            let path = fixture.write_file("R/main.R", &first).unwrap();
            let uri = file_uri(&path).unwrap();
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;

            let mark = session.publication_mark();
            session.open(&uri, 1, &first).await.unwrap();
            let initial = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            assert_eq!(count_code(&initial, "RY114"), 1, "{initial}");
            let mismatch = initial["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .find(|diagnostic| diagnostic["code"] == "RY114")
                .unwrap();
            assert_eq!(
                mismatch["range"]["start"],
                json!({"line": 5, "character": 2})
            );

            for (version, annotation, expected) in [
                (2, "#| x character", 0),
                (3, "# ordinary comment", 0),
                (4, "#| x integer", 1),
                (5, "NULL #| x integer", 0),
                (6, "#| x integer", 1),
            ] {
                let mark = session.publication_mark();
                session
                    .change(&uri, version, json!([{"text": source(annotation)}]))
                    .await
                    .unwrap();
                let publish = session
                    .published_diagnostics_after(&uri, mark)
                    .await
                    .unwrap();
                assert_eq!(count_code(&publish, "RY114"), expected, "{publish}");
            }
            let config_uri = file_uri(&fixture.path("ry.toml")).unwrap();
            for (config, expected) in [
                (
                    "[annotations.typehint]\nadopt = false\nversion = '0.1.0'\npaths = ['R/**']\n",
                    0,
                ),
                (
                    "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['other/**']\n",
                    0,
                ),
                (
                    "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
                    1,
                ),
            ] {
                fixture.write_file("ry.toml", config).unwrap();
                sync_barrier(&mut session, &uri).await;
                let mark = session.publication_mark();
                session
                    .notify(
                        "workspace/didChangeWatchedFiles",
                        json!({"changes": [{"uri": config_uri, "type": 2}]}),
                    )
                    .await
                    .unwrap();
                let publish = session
                    .published_diagnostics_after(&uri, mark)
                    .await
                    .unwrap();
                assert_eq!(count_code(&publish, "RY114"), expected, "{publish}");
            }
            join_session(session, server).await;
        });
}
