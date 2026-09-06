mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

#[test]
fn suppression_edits_use_the_source_version_when_supported() {
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
                    |supported| json!({"workspace": {"workspaceEdit": {"documentChanges": supported}}}),
                );
                let (mut session, server) = spawn_session(&[fixture.root()], capabilities, None).await;
                let mut previous_actions = Value::Null;
                for (version, source) in [(4, "x <- unknown_name\n"), (5, "longer_name <- unknown_name\n")] {
                    let mark = session.publication_mark();
                    if version == 4 {
                        session.open(&uri, version, source).await.unwrap();
                    } else {
                        session.change(&uri, version, json!([{"text": source}])).await.unwrap();
                    }
                    let published = session.published_diagnostics_after(&uri, mark).await.unwrap();
                    let diagnostics = normalize_diagnostics(&published);
                    let diagnostic = &diagnostics[0];
                    let actions = session.request("textDocument/codeAction", json!({
                        "textDocument": {"uri": uri},
                        "range": diagnostic["range"],
                        "context": {"diagnostics": [diagnostic]}
                    })).await.unwrap();
                    let entries = actions.as_array().unwrap();
                    assert_eq!(entries.len(), 2, "{actions}");
                    for (index, action) in entries.iter().enumerate() {
                        let edits = if support == Some(true) {
                            assert!(action["edit"].get("changes").is_none(), "{action}");
                            let changes = action["edit"]["documentChanges"].as_array().unwrap();
                            assert_eq!(changes.len(), 1);
                            assert_eq!(changes[0]["textDocument"], json!({"uri": uri, "version": version}));
                            &changes[0]["edits"]
                        } else {
                            assert!(action["edit"].get("documentChanges").is_none(), "{action}");
                            &action["edit"]["changes"][&uri]
                        };
                        assert_eq!(edits.as_array().unwrap().len(), 1);
                        if index == 0 {
                            assert_eq!(edits[0]["range"]["end"]["character"], source.trim_end().len());
                            assert_eq!(edits[0]["newText"], format!("{}  # ry: ignore[RY010]", source.trim_end()));
                        } else {
                            assert_eq!(edits[0]["newText"], "# ry: ignore-file\n");
                        }
                    }
                    if version == 5 && support == Some(true) {
                        // A saved action still names version 4, so the client can
                        // reject it instead of replacing version 5's longer line.
                        assert_eq!(previous_actions[0]["edit"]["documentChanges"][0]["textDocument"]["version"], 4);
                    }
                    previous_actions = actions;
                }
                join_session(session, server).await;
            }
        });
}
