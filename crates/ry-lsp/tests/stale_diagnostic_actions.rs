mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

#[test]
fn suppression_requests_reject_stale_diagnostic_origins_when_supported() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for support in [None, Some(false), Some(true)] {
                let fixture = FixtureProject::empty().unwrap();
                let uri = file_uri(&fixture.write_file("main.R", "x <- unknown_name\n").unwrap());
                let capabilities = support.map_or_else(
                    || json!({}),
                    |supported| json!({"textDocument": {"publishDiagnostics": {"dataSupport": supported}}}),
                );
                let (mut session, server) = spawn_session(&[fixture.root()], capabilities, None).await;
                let mark = session.publication_mark();
                session.open(&uri, 4, "x <- unknown_name\n").await.unwrap();
                let first = session.published_diagnostics_after(&uri, mark).await.unwrap();
                let old_diagnostic = normalize_diagnostics(&first).remove(0);

                let mark = session.publication_mark();
                session.change(&uri, 5, json!([{"text": "unrelated <- 1L\nx <- unknown_name\n"}])).await.unwrap();
                let second = session.published_diagnostics_after(&uri, mark).await.unwrap();
                let current_diagnostic = normalize_diagnostics(&second).remove(0);
                let request = |diagnostics: Vec<Value>| json!({
                    "textDocument": {"uri": uri},
                    "range": current_diagnostic["range"],
                    "context": {"diagnostics": diagnostics}
                });
                let stale_actions = session.request("textDocument/codeAction", request(vec![old_diagnostic.clone()])).await.unwrap();
                if support == Some(true) {
                    assert!(stale_actions.is_null(), "{stale_actions}");
                    assert_eq!(current_diagnostic["data"]["ry"]["version"], 5);
                    for data in [None, Some(json!({"invalid": true}))] {
                        let mut malformed = current_diagnostic.clone();
                        malformed.as_object_mut().unwrap().remove("data");
                        if let Some(data) = data {
                            malformed["data"] = data;
                        }
                        let actions = session.request("textDocument/codeAction", request(vec![malformed])).await.unwrap();
                        assert!(actions.is_null(), "{actions}");
                    }
                    let mixed = session.request("textDocument/codeAction", request(vec![old_diagnostic, current_diagnostic.clone()])).await.unwrap();
                    assert_eq!(mixed.as_array().unwrap().len(), 2, "{mixed}");
                    assert_eq!(mixed[0]["edit"]["changes"][&uri][0]["range"]["start"]["line"], 1);
                } else {
                    // Legacy clients never promised to preserve diagnostic data.
                    assert_eq!(stale_actions.as_array().unwrap().len(), 2);
                    assert!(current_diagnostic.get("data").is_none());
                }
                let actions = session.request("textDocument/codeAction", request(vec![current_diagnostic.clone()])).await.unwrap();
                assert_eq!(actions.as_array().unwrap().len(), 2, "{actions}");
                assert_eq!(actions[0]["edit"]["changes"][&uri][0]["newText"], "x <- unknown_name  # ry: ignore[RY010]");

                if support == Some(true) {
                    // A settings reload changes analysis without changing text.
                    let mark = session.publication_mark();
                    session.notify("workspace/didChangeConfiguration", json!({"settings": {"ry": {"enable": true}}})).await.unwrap();
                    let refreshed = session.published_diagnostics_after(&uri, mark).await.unwrap();
                    let refreshed_diagnostic = normalize_diagnostics(&refreshed).remove(0);
                    assert_eq!(refreshed_diagnostic["data"]["ry"]["version"], 5);
                    assert_ne!(refreshed_diagnostic["data"], current_diagnostic["data"]);
                    let stale = session.request("textDocument/codeAction", request(vec![current_diagnostic.clone()])).await.unwrap();
                    assert!(stale.is_null(), "{stale}");
                    let fresh = session.request("textDocument/codeAction", request(vec![refreshed_diagnostic])).await.unwrap();
                    assert_eq!(fresh.as_array().unwrap().len(), 2, "{fresh}");
                }
                join_session(session, server).await;
            }
        });
}
