//! The line quick-fix through the protocol: a repair keeps the existing
//! comment and removes only its target, and a malformed directive withholds
//! the repair. The marker table lives with the unit tests.

mod harness;

use harness::{ClientSession, file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

async fn line_action(session: &mut ClientSession, uri: &str, diagnostic: &Value) -> Option<Value> {
    let code = diagnostic["code"].as_str().unwrap();
    let request = json!({
        "textDocument": {"uri": uri},
        "range": diagnostic["range"],
        "context": {"diagnostics": [diagnostic]}
    });
    let actions = session
        .request("textDocument/codeAction", request)
        .await
        .unwrap();
    actions.as_array().and_then(|actions| {
        actions
            .iter()
            .find(|action| action["title"] == format!("Ignore {code} on this line"))
            .cloned()
    })
}

#[tokio::test]
async fn suppression_edit_keeps_the_comment_and_malformed_directives_withhold_it() {
    let source = "x <- never_bound_here # explanation\n\"a\" + 1L # ry: ignore[RY040]]\n";
    let fixture = FixtureProject::empty().unwrap();
    let uri = file_uri(&fixture.write_file("main.R", source).unwrap());
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, source).await.unwrap();
    let before = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    let diagnostics = normalize_diagnostics(&before);
    let find = |code: &str| {
        diagnostics
            .iter()
            .find(|diag| diag["code"] == code)
            .unwrap_or_else(|| panic!("{code}: {before}"))
            .clone()
    };
    let unbound = find("RY010");
    let malformed = find("RY040");

    let mut foreign = unbound.clone();
    foreign["source"] = json!("lintr");
    assert!(line_action(&mut session, &uri, &foreign).await.is_none());
    assert!(line_action(&mut session, &uri, &malformed).await.is_none());

    let action = line_action(&mut session, &uri, &unbound)
        .await
        .expect("repair action");
    let edit = &action["edit"]["changes"][&uri][0];
    let old_line = source.lines().next().unwrap();
    assert_eq!(edit["range"]["start"], json!({"line": 0, "character": 0}));
    assert_eq!(
        edit["range"]["end"],
        json!({"line": 0, "character": old_line.len()})
    );
    let new_line = edit["newText"].as_str().unwrap();
    assert!(new_line.contains("# explanation"), "{new_line}");
    let updated = source.replacen(old_line, new_line, 1);
    let mark = session.publication_mark();
    session
        .change(&uri, 2, json!([{"text": updated}]))
        .await
        .unwrap();
    let after = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    let expected: Vec<_> = diagnostics
        .iter()
        .filter(|diag| **diag != unbound)
        .cloned()
        .collect();
    assert_eq!(normalize_diagnostics(&after), expected, "{updated}");
    join_session(session, server).await;
}
