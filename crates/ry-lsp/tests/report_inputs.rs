//! The editor checks the original report coordinates through the shared mask.
mod harness;

use harness::{join_session, spawn_session, sync_barrier};
use ry_testkit::{FixtureProject, file_uri};
use serde_json::{Value, json};

fn finding<'a>(publish: &'a Value, code: &str) -> Option<&'a Value> {
    publish["params"]["diagnostics"]
        .as_array()?
        .iter()
        .find(|diag| diag["code"] == code)
}

#[test]
fn warm_report_prose_and_option_edits_keep_original_utf16_positions() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture.write_file("ry.toml", "[reports]\nenabled = true\n").unwrap();
            let report = |prose: &str, eval: &str| format!(
                "{prose}\r\n```{{r}}\r\nx <- \"a\"\r\n```\r\n```{{r}}\r\n#| eval: {eval}\r\nx + 1L\r\n```\r\n"
            );
            let first = report("😀 prose", "true");
            let uri = file_uri(&fixture.write_file("memo.qmd", &first).unwrap()).unwrap();
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
            let mark = session.publication_mark();
            session.open(&uri, 1, &first).await.unwrap();
            let published = session.published_diagnostics_after(&uri, mark).await.unwrap();
            let bad = finding(&published, "RY040").expect("active later chunk: RY040");
            assert_eq!(bad["range"]["start"], json!({"line": 6, "character": 0}), "{published}");
            let action = session.request("textDocument/codeAction", json!({
                "textDocument": {"uri": uri},
                "range": bad["range"],
                "context": {"diagnostics": [bad], "only": ["quickfix"]},
            })).await.unwrap();
            assert!(action.is_null(), "report edits are refused: {action}");

            // A prose-only UTF-16 range edit changes two surrogate units but
            // leaves the R chunk at its original line and byte coordinates.
            let mark = session.publication_mark();
            session.change(&uri, 2, json!([{"range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}
            }, "text": "界"}])).await.unwrap();
            let prose = session.published_diagnostics_after(&uri, mark).await.unwrap();
            assert_eq!(finding(&prose, "RY040").unwrap()["range"]["start"], json!({"line": 6, "character": 0}), "{prose}");

            let mark = session.publication_mark();
            session.change(&uri, 3, json!([{"text": report("界 prose", "false")}])).await.unwrap();
            let disabled = session.published_diagnostics_after(&uri, mark).await.unwrap();
            assert!(finding(&disabled, "RY040").is_none(), "{disabled}");
            let mark = session.publication_mark();
            session.change(&uri, 4, json!([{"text": report("界 prose\r\nextra prose", "true")}])).await.unwrap();
            let moved = session.published_diagnostics_after(&uri, mark).await.unwrap();
            assert_eq!(finding(&moved, "RY040").unwrap()["range"]["start"], json!({"line": 7, "character": 0}), "{moved}");
            let mark = session.publication_mark();
            session.change(&uri, 5, json!([{"text": "界 prose\r\n```{r}\r\nx <- (\r\n```\r\n"}])).await.unwrap();
            let broken = session.published_diagnostics_after(&uri, mark).await.unwrap();
            assert_eq!(finding(&broken, "RY000").unwrap()["range"]["start"], json!({"line": 2, "character": 2}), "{broken}");
            assert!(finding(&broken, "RY040").is_none(), "{broken}");
            join_session(session, server).await;
        });
}

#[test]
fn report_config_toggle_rechecks_an_open_buffer() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file("ry.toml", "[reports]\nenabled = false\n")
                .unwrap();
            let source = "```{r}\n\"a\" + 1L\n```\n";
            let uri = file_uri(&fixture.write_file("memo.qmd", source).unwrap()).unwrap();
            let config_uri = file_uri(&fixture.path("ry.toml")).unwrap();
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
            let mark = session.publication_mark();
            session.open(&uri, 1, source).await.unwrap();
            let initial = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            assert!(finding(&initial, "RY040").is_none(), "{initial}");
            fixture
                .write_file("ry.toml", "[reports]\nenabled = true\n")
                .unwrap();
            sync_barrier(&mut session, &uri).await;
            let mark = session.publication_mark();
            session
                .notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": config_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            let enabled = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            assert!(finding(&enabled, "RY040").is_some(), "{enabled}");
            fixture
                .write_file("ry.toml", "[reports]\nenabled = false\n")
                .unwrap();
            sync_barrier(&mut session, &uri).await;
            let mark = session.publication_mark();
            session
                .notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": config_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            let disabled = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            assert!(finding(&disabled, "RY040").is_none(), "{disabled}");
            join_session(session, server).await;
        });
}
