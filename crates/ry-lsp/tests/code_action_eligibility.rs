mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

fn request(uri: &str, diagnostic: &Value) -> Value {
    json!({
        "textDocument": {"uri": uri},
        "range": diagnostic["range"],
        "context": {"diagnostics": [diagnostic]}
    })
}

#[test]
fn suppression_actions_respect_requested_kinds() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            let source = "x <- never_bound_here\n";
            let uri = file_uri(&fixture.write_file("main.R", source).unwrap());
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
            let mark = session.publication_mark();
            session.open(&uri, 1, source).await.unwrap();
            let published = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            let diagnostics = normalize_diagnostics(&published);
            let base = request(&uri, &diagnostics[0]);
            for (only, expected) in [
                (None, true),
                (Some(json!(["quickfix"])), true),
                (Some(json!([""])), true),
                (Some(json!(["refactor", "quickfix"])), true),
                (Some(json!([])), false),
                (Some(json!(["source.fixAll"])), false),
                (Some(json!(["refactor"])), false),
                (Some(json!(["quickfix.custom"])), false),
            ] {
                let mut params = base.clone();
                if let Some(only) = only {
                    params["context"]["only"] = only;
                }
                let actions = session
                    .request("textDocument/codeAction", params.clone())
                    .await
                    .unwrap();
                if expected {
                    let actions = actions.as_array().unwrap();
                    assert_eq!(actions.len(), 2, "{params}");
                    assert!(actions.iter().all(|action| action["kind"] == "quickfix"));
                } else {
                    assert!(actions.is_null(), "{params}: {actions}");
                }
            }
            join_session(session, server).await;
        });
}

#[test]
fn stale_diagnostics_do_not_offer_suppressions_after_disabling_or_excluding() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for exclude in [false, true] {
                let fixture = FixtureProject::empty().unwrap();
                let source = "x <- never_bound_here\n";
                let uri = file_uri(&fixture.write_file("main.R", source).unwrap());
                let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
                let mark = session.publication_mark();
                session.open(&uri, 1, source).await.unwrap();
                let published = session
                    .published_diagnostics_after(&uri, mark)
                    .await
                    .unwrap();
                let diagnostics = normalize_diagnostics(&published);
                let params = request(&uri, &diagnostics[0]);
                let before = session
                    .request("textDocument/codeAction", params.clone())
                    .await
                    .unwrap();
                assert_eq!(before.as_array().unwrap().len(), 2);
                let settings = if exclude {
                    fixture
                        .write_file("ry.toml", "exclude = [\"main.R\"]\n")
                        .unwrap();
                    json!({})
                } else {
                    json!({"ry": {"enable": false}})
                };
                let mark = session.publication_mark();
                session
                    .notify(
                        "workspace/didChangeConfiguration",
                        json!({"settings": settings}),
                    )
                    .await
                    .unwrap();
                let refreshed = session
                    .published_diagnostics_after(&uri, mark)
                    .await
                    .unwrap();
                assert!(normalize_diagnostics(&refreshed).is_empty(), "{refreshed}");
                let after = session
                    .request("textDocument/codeAction", params)
                    .await
                    .unwrap();
                assert!(after.is_null(), "exclude={exclude}: {after}");
                join_session(session, server).await;
            }
        });
}
