//! The line quick-fix through the protocol: a repair keeps the existing
//! comment and reason, merges into an existing rule list, repairs an invalid
//! code, and removes only its target; a malformed directive withholds it.
//! The marker table lives with the unit tests.

use crate::harness;

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

fn find(diagnostics: &[Value], code: &str, line: u64) -> Value {
    diagnostics
        .iter()
        .find(|diag| diag["code"] == code && diag["range"]["start"]["line"] == line)
        .unwrap_or_else(|| panic!("{code} on line {line}: {diagnostics:?}"))
        .clone()
}

#[tokio::test]
async fn suppression_edits_repair_merge_and_withhold() {
    let mut text = concat!(
        "x <- never_bound_here # explanation\n",
        "\"a\" + 1L # ry: ignore[RY040]]\n",
        "y <- 1L + \"s\"; z <- never_bound_here # ry: ignore[RY040] reason\n",
        "\"b\" + 1L # ry: ignore[RX040] reason\n",
    )
    .to_string();
    let fixture = FixtureProject::empty().unwrap();
    let uri = file_uri(&fixture.write_file("main.R", &text).unwrap());
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, &text).await.unwrap();
    let publish = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    let mut diagnostics = normalize_diagnostics(&publish);

    let mut foreign = find(&diagnostics, "RY010", 0);
    foreign["source"] = json!("lintr");
    assert!(line_action(&mut session, &uri, &foreign).await.is_none());
    let malformed = find(&diagnostics, "RY040", 1);
    assert!(line_action(&mut session, &uri, &malformed).await.is_none());

    for (version, (code, line, expected)) in [
        ("RY010", 0, "# explanation"),
        ("RY010", 2, "# ry: ignore[RY010, RY040] reason"),
        ("RY040", 3, "# ry: ignore[RY040] reason"),
    ]
    .into_iter()
    .enumerate()
    {
        let target = find(&diagnostics, code, line);
        let action = line_action(&mut session, &uri, &target)
            .await
            .unwrap_or_else(|| panic!("{code} on line {line}"));
        let edit = &action["edit"]["changes"][&uri][0];
        let old_line = text.lines().nth(line as usize).unwrap().to_string();
        assert_eq!(
            edit["range"]["start"],
            json!({"line": line, "character": 0})
        );
        assert_eq!(
            edit["range"]["end"],
            json!({"line": line, "character": old_line.encode_utf16().count()})
        );
        let new_line = edit["newText"].as_str().unwrap();
        assert!(new_line.contains(expected), "{new_line}");
        assert_eq!(new_line.matches("# ry: ignore[").count(), 1);
        text = text.replacen(&old_line, new_line, 1);
        let mark = session.publication_mark();
        session
            .change(&uri, version as i32 + 2, json!([{"text": text}]))
            .await
            .unwrap();
        let publish = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        let after = normalize_diagnostics(&publish);
        // Only the target leaves, plus the RY112 of the repaired directive.
        let expected: Vec<_> = diagnostics
            .iter()
            .filter(|diag| {
                **diag != target
                    && !(diag["code"] == "RY112" && diag["range"]["start"]["line"] == line)
            })
            .cloned()
            .collect();
        assert_eq!(after, expected, "{text}");
        diagnostics = after;
    }
    join_session(session, server).await;
}
