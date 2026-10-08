mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::json;

#[test]
fn suppression_edits_remove_only_the_target_and_preserve_source() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        for (source, target_line, code, editable) in [
            ("x <- never_bound_here # explanation\ny <- other\n", 0, "RY010", true),
            ("x <- never_bound_here # noqa: RY040 [note]\ny <- other\n", 0, "RY010", true),
            ("x <- never_bound_here # noqa RY040 reason\ny <- other\n", 0, "RY010", true),
            ("x <- never_bound_here # ry:ignore RY040 reason\ny <- other\n", 0, "RY010", true),
            ("x <- 1L + \"s\"; y <- never_bound_here # ry: ignore[RY040] reason\nz <- other\n", 0, "RY010", true),
            ("x <- \"café # text\nlast\"\ny <- never_bound_here # explanation\nz <- other\n", 2, "RY010", true),
            ("x <- \"first\nlast\" + 1L\ny <- other\n", 0, "RY040", false),
        ] {
            let fixture = FixtureProject::empty().unwrap();
            let path = fixture.write_file("main.R", source).unwrap();
            let uri = file_uri(&path);
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
            let mark = session.publication_mark();
            session.open(&uri, 1, source).await.unwrap();
            let before = session.published_diagnostics_after(&uri, mark).await.unwrap();
            let diagnostics = normalize_diagnostics(&before);
            let diagnostic = diagnostics.iter().find(|d| d["code"] == code && d["range"]["start"]["line"] == target_line).unwrap_or_else(|| panic!("target diagnostic: {before}"));
            let request = json!({"textDocument":{"uri":uri}, "range":diagnostic["range"], "context":{"diagnostics":[diagnostic]}});
            let mut foreign = request.clone();
            foreign["context"]["diagnostics"][0]["source"] = json!("lintr");
            assert!(session.request("textDocument/codeAction", foreign).await.unwrap().is_null());
            let actions = session.request("textDocument/codeAction", request).await.unwrap();
            let title = format!("Ignore {code} on this line");
            let action = actions.as_array().unwrap().iter().find(|action| action["title"] == title);
            assert_eq!(action.is_some(), editable, "{actions}");
            if let Some(action) = action {
                let edit = &action["edit"]["changes"][&uri][0];
                let line = target_line as usize;
                let old_line = source.lines().nth(line).unwrap();
                assert_eq!(edit["range"]["start"], json!({"line":line,"character":0}));
                assert_eq!(edit["range"]["end"], json!({"line":line,"character":old_line.encode_utf16().count()}));
                let start: usize = source.split_inclusive('\n').take(line).map(str::len).sum();
                let mut updated = source.to_string();
                updated.replace_range(start..start + old_line.len(), edit["newText"].as_str().unwrap());
                if old_line.contains("# ry: ignore[") {
                    assert!(updated.contains("# ry: ignore[RY010, RY040] reason"));
                    assert_eq!(updated.matches("# ry: ignore[").count(), 1);
                } else if let Some(comment) = old_line.find("# ") {
                    assert!(updated.contains(&old_line[comment..]));
                }
                assert_eq!(&updated[..start], &source[..start]);
                let mark = session.publication_mark();
                session.change(&uri, 2, json!([{"text":updated}])).await.unwrap();
                let after = session.published_diagnostics_after(&uri, mark).await.unwrap();
                let expected: Vec<_> = diagnostics.iter().filter(|d| *d != diagnostic).cloned().collect();
                assert_eq!(normalize_diagnostics(&after), expected, "{updated}");
            }
            join_session(session, server).await;
        }
    });
}

#[test]
fn malformed_directive_actions_either_repair_or_withhold() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        for (marker, repairs) in [
            ("ry: ignore[RX040] reason", true),
            ("ry: ignore[RY040]]", false),
            ("ry: ignore[RY040] ]", false),
            ("noqa[RY040]]", false),
        ] {
            let source = format!("\"a\" + 1L # {marker}\n");
            let fixture = FixtureProject::empty().unwrap();
            let path = fixture.write_file("main.R", &source).unwrap();
            let uri = file_uri(&path);
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
            let mark = session.publication_mark();
            session.open(&uri, 1, &source).await.unwrap();
            let before = session.published_diagnostics_after(&uri, mark).await.unwrap();
            let diagnostics = normalize_diagnostics(&before);
            let target = diagnostics.iter().find(|diag| diag["code"] == "RY040").unwrap_or_else(|| panic!("{marker}: {before}"));
            let request = json!({"textDocument":{"uri":uri}, "range":target["range"], "context":{"diagnostics":[target]}});
            let actions = session.request("textDocument/codeAction", request).await.unwrap();
            let action = actions.as_array().and_then(|actions| actions.iter().find(|action| action["title"] == "Ignore RY040 on this line"));
            assert_eq!(action.is_some(), repairs, "{marker}: {actions}");
            if let Some(action) = action {
                let new_text = action["edit"]["changes"][&uri][0]["newText"].as_str().unwrap();
                assert!(new_text.ends_with("# ry: ignore[RY040] reason"));
                let updated = format!("{new_text}\n");
                let mark = session.publication_mark();
                session.change(&uri, 2, json!([{"text":updated}])).await.unwrap();
                let after = session.published_diagnostics_after(&uri, mark).await.unwrap();
                let after = normalize_diagnostics(&after);
                assert!(!after.iter().any(|diag| diag["code"] == "RY040" || diag["code"] == "RY112"), "{marker}: {after:?}");
            }
            join_session(session, server).await;
        }
    });
}
