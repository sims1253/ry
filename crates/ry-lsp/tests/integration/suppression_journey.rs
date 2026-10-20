//! Suppression quick fixes through the protocol, once per client: no
//! capabilities, and `workspaceEdit.documentChanges` with
//! `publishDiagnostics.dataSupport` both off and both on.

use crate::harness;

use harness::{ClientSession, file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

const FILE_ACTION: &str = "Ignore all diagnostics in this file";

struct Journey {
    session: ClientSession,
    fixture: FixtureProject,
    uri: String,
    version: i32,
    /// The client accepts versioned `documentChanges` edits and sends
    /// diagnostic `data` back with code action requests.
    capable: bool,
}

impl Journey {
    /// Replace the document text and return its next diagnostics.
    async fn replace(&mut self, text: &str) -> Vec<Value> {
        self.version += 1;
        let publish =
            harness::change_published(&mut self.session, &self.uri, self.version, text).await;
        normalize_diagnostics(&publish)
    }

    /// Send a configuration change and return the next diagnostics.
    async fn configure(&mut self, settings: Value) -> Vec<Value> {
        let mark = self.session.publication_mark();
        self.session
            .notify(
                "workspace/didChangeConfiguration",
                json!({"settings": settings}),
            )
            .await
            .unwrap();
        let publish = self
            .session
            .published_diagnostics_after(&self.uri, mark)
            .await
            .unwrap();
        normalize_diagnostics(&publish)
    }

    async fn request(
        &mut self,
        range: &Value,
        diagnostics: &[Value],
        only: Option<Value>,
    ) -> Value {
        let mut params = json!({
            "textDocument": {"uri": self.uri},
            "range": range,
            "context": {"diagnostics": diagnostics}
        });
        if let Some(only) = only {
            params["context"]["only"] = only;
        }
        self.session
            .request("textDocument/codeAction", params)
            .await
            .unwrap()
    }

    /// The actions offered for one diagnostic over its own range.
    async fn actions(&mut self, diagnostic: &Value) -> Value {
        let range = diagnostic["range"].clone();
        self.request(&range, std::slice::from_ref(diagnostic), None)
            .await
    }

    /// The line action offered for `diagnostic`, if any.
    async fn line_action(&mut self, diagnostic: &Value) -> Option<Value> {
        let title = format!(
            "Ignore {} on this line",
            diagnostic["code"].as_str().unwrap()
        );
        let actions = self.actions(diagnostic).await;
        actions
            .as_array()?
            .iter()
            .find(|action| action["title"] == title)
            .cloned()
    }

    /// The single text edit of `action`, in the shape the client
    /// negotiated: versioned `documentChanges` naming the current version,
    /// or plain `changes`.
    fn edit(&self, action: &Value) -> Value {
        let edit = &action["edit"];
        let edits = if self.capable {
            assert!(edit.get("changes").is_none(), "{action}");
            let changes = edit["documentChanges"].as_array().unwrap();
            assert_eq!(changes.len(), 1, "{action}");
            assert_eq!(
                changes[0]["textDocument"],
                json!({"uri": self.uri, "version": self.version})
            );
            &changes[0]["edits"]
        } else {
            assert!(edit.get("documentChanges").is_none(), "{action}");
            &edit["changes"][&self.uri]
        };
        let edits = edits.as_array().unwrap();
        assert_eq!(edits.len(), 1, "{action}");
        edits[0].clone()
    }

    /// Diagnostics without their opaque `data`, which changes on every
    /// analysis.
    fn without_data(diagnostics: &[Value]) -> Vec<Value> {
        diagnostics
            .iter()
            .map(|diagnostic| {
                let mut diagnostic = diagnostic.clone();
                diagnostic.as_object_mut().unwrap().remove("data");
                diagnostic
            })
            .collect()
    }
}

fn find(diagnostics: &[Value], code: &str, line: u64) -> Value {
    diagnostics
        .iter()
        .find(|diag| diag["code"] == code && diag["range"]["start"]["line"] == line)
        .unwrap_or_else(|| panic!("{code} on line {line}: {diagnostics:?}"))
        .clone()
}

#[tokio::test]
async fn suppression_actions_follow_the_document_and_the_client_capabilities() {
    for enabled in [None, Some(false), Some(true)] {
        let capabilities = enabled.map_or_else(
            || json!({}),
            |enabled| {
                json!({
                    "workspace": {"workspaceEdit": {"documentChanges": enabled}},
                    "textDocument": {"publishDiagnostics": {"dataSupport": enabled}}
                })
            },
        );
        let fixture = FixtureProject::empty().unwrap();
        let source = "x <- never_bound_here\n";
        let uri = file_uri(&fixture.write_file("main.R", source).unwrap());
        let (session, server) = spawn_session(&[fixture.root()], capabilities, None).await;
        let mut journey = Journey {
            session,
            fixture,
            uri,
            version: 1,
            capable: enabled == Some(true),
        };
        let publish = harness::open_published(&mut journey.session, &journey.uri, 1, source).await;
        let diagnostics = normalize_diagnostics(&publish);

        only_kinds(&mut journey, &diagnostics[0]).await;
        repair_merge_and_withhold(&mut journey).await;
        file_actions_follow_the_comments(&mut journey).await;
        stale_diagnostics_get_no_actions(&mut journey).await;
        unused_ignores_track_edits(&mut journey).await;
        closed_and_disabled_documents_get_no_actions(&mut journey).await;

        join_session(journey.session, server).await;
    }
}

/// Actions are quick fixes: an `only` filter must admit `quickfix`.
async fn only_kinds(journey: &mut Journey, diagnostic: &Value) {
    for (only, offered) in [
        (None, true),
        (Some(json!(["quickfix"])), true),
        (Some(json!([""])), true),
        (Some(json!(["refactor", "quickfix"])), true),
        (Some(json!([])), false),
        (Some(json!(["source.fixAll"])), false),
        (Some(json!(["refactor"])), false),
        (Some(json!(["quickfix.custom"])), false),
    ] {
        let label = format!("{only:?}");
        let actions = journey
            .request(&diagnostic["range"], std::slice::from_ref(diagnostic), only)
            .await;
        if offered {
            let actions = actions.as_array().unwrap();
            assert_eq!(actions.len(), 2, "{label}");
            assert!(actions.iter().all(|action| action["kind"] == "quickfix"));
        } else {
            assert!(actions.is_null(), "{label}: {actions}");
        }
    }
}

/// The line fix keeps an existing comment and reason, merges into an
/// existing rule list, repairs an invalid code, and removes only its target.
/// A malformed directive and a foreign diagnostic get no line fix.
async fn repair_merge_and_withhold(journey: &mut Journey) {
    let mut text = concat!(
        "x <- never_bound_here # explanation\n",
        "\"a\" + 1L # ry: ignore[RY040]]\n",
        "y <- 1L + \"s\"; z <- never_bound_here # ry: ignore[RY040] reason\n",
        "\"b\" + 1L # ry: ignore[RX040] reason\n",
    )
    .to_string();
    let mut diagnostics = journey.replace(&text).await;

    let mut foreign = find(&diagnostics, "RY010", 0);
    foreign["source"] = json!("lintr");
    assert!(journey.line_action(&foreign).await.is_none());
    let malformed = find(&diagnostics, "RY040", 1);
    assert!(journey.line_action(&malformed).await.is_none());

    for (code, line, expected) in [
        ("RY010", 0, "# explanation"),
        ("RY010", 2, "# ry: ignore[RY010, RY040] reason"),
        ("RY040", 3, "# ry: ignore[RY040] reason"),
    ] {
        let target = find(&diagnostics, code, line);
        let action = journey
            .line_action(&target)
            .await
            .unwrap_or_else(|| panic!("{code} on line {line}"));
        let edit = journey.edit(&action);
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
        let after = journey.replace(&text).await;
        // Only the target leaves, plus the RY112 of the repaired directive.
        let expected: Vec<_> = diagnostics
            .iter()
            .filter(|diag| {
                **diag != target
                    && !(diag["code"] == "RY112" && diag["range"]["start"]["line"] == line)
            })
            .cloned()
            .collect();
        assert_eq!(
            Journey::without_data(&after),
            Journey::without_data(&expected),
            "{text}"
        );
        diagnostics = after;
    }
}

/// The file action is offered unless the current text already carries a
/// file directive. Lookalikes in strings and prose are not directives.
async fn file_actions_follow_the_comments(journey: &mut Journey) {
    for (text, offered) in [
        (
            "x <- \"# ry: ignore-file\"\ny <- never_defined_name\n",
            true,
        ),
        ("# RY:IGNORE-FILE\ny <- never_defined_name\n", false),
        (
            "# See ry: ignore-file in the docs\ny <- never_defined_name\n",
            true,
        ),
    ] {
        let diagnostics = journey.replace(text).await;
        assert_eq!(!diagnostics.is_empty(), offered);
        let range =
            json!({"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}});
        let actions = journey.request(&range, &diagnostics, None).await;
        let file_action = actions
            .as_array()
            .and_then(|actions| actions.iter().find(|action| action["title"] == FILE_ACTION));
        assert_eq!(file_action.is_some(), offered, "{text}");
        if let Some(action) = file_action {
            assert_eq!(journey.edit(action)["newText"], "# ry: ignore-file\n");
        }
    }
}

/// Edits name the version they were computed for. A diagnostic from an
/// older analysis gets no actions when the client returns diagnostic data,
/// so a stale range cannot suppress an unrelated current line. Clients
/// without data support get actions for any range.
async fn stale_diagnostics_get_no_actions(journey: &mut Journey) {
    let check_edit = |line: &str, edit: Value, action_index: usize| {
        if action_index == 0 {
            assert_eq!(edit["range"]["end"]["character"], line.len());
            assert_eq!(edit["newText"], format!("{line}  # ry: ignore[RY010]"));
        } else {
            assert_eq!(edit["newText"], "# ry: ignore-file\n");
        }
    };
    let old = journey.replace("x <- unknown_name\n").await.remove(0);
    let actions = journey.actions(&old).await;
    let actions = actions.as_array().unwrap();
    assert_eq!(actions.len(), 2, "{actions:?}");
    for (index, action) in actions.iter().enumerate() {
        check_edit("x <- unknown_name", journey.edit(action), index);
    }

    let current = journey
        .replace("unrelated <- 1L\nlonger_name <- unknown_name\n")
        .await
        .remove(0);
    let actions = journey.actions(&current).await;
    let actions = actions.as_array().unwrap();
    assert_eq!(actions.len(), 2, "{actions:?}");
    for (index, action) in actions.iter().enumerate() {
        check_edit("longer_name <- unknown_name", journey.edit(action), index);
    }
    assert_eq!(journey.edit(&actions[0])["range"]["start"]["line"], 1);

    let range = current["range"].clone();
    let stale = journey
        .request(&range, std::slice::from_ref(&old), None)
        .await;
    if !journey.capable {
        // Legacy clients never promised to preserve diagnostic data.
        assert_eq!(stale.as_array().unwrap().len(), 2, "{stale}");
        assert!(current.get("data").is_none());
        return;
    }
    assert!(stale.is_null(), "{stale}");
    for data in [None, Some(json!({"invalid": true}))] {
        let mut malformed = current.clone();
        malformed.as_object_mut().unwrap().remove("data");
        if let Some(data) = data {
            malformed["data"] = data;
        }
        let actions = journey.request(&range, &[malformed], None).await;
        assert!(actions.is_null(), "{actions}");
    }
    let mixed = journey.request(&range, &[old, current.clone()], None).await;
    assert_eq!(mixed.as_array().unwrap().len(), 2, "{mixed}");
    assert_eq!(journey.edit(&mixed[0])["range"]["start"]["line"], 1);

    // A settings reload changes the analysis without changing the text.
    let refreshed = journey
        .configure(json!({"ry": {"enable": true}}))
        .await
        .remove(0);
    let stale = journey
        .request(&range, std::slice::from_ref(&current), None)
        .await;
    assert!(stale.is_null(), "{stale}");
    let fresh = journey.actions(&refreshed).await;
    assert_eq!(fresh.as_array().unwrap().len(), 2, "{fresh}");
}

/// With RY113 enabled, an unused directive is reported on its own line as
/// edits fix, move, and remove the finding it suppressed.
async fn unused_ignores_track_edits(journey: &mut Journey) {
    let used = "1L == NA # ry: ignore[RY034]\n";
    journey.replace(used).await;
    journey
        .fixture
        .write_file("ry.toml", "warn = [\"RY113\"]\n")
        .unwrap();
    let diagnostics = journey.configure(json!({})).await;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    for (source, expected_line) in [
        ("1L == 1L # ry: ignore[RY034]\n", Some(0)),
        ("# heading\n# ry: ignore[RY034]\n1L == 1L\n", Some(1)),
        ("1L == 1L\n", None),
        (
            "identity(function() {\n  1L == NA # ry: ignore[RY034]\n})\n",
            None,
        ),
        (
            "identity(function() {\n  list(\"a\" <- 1L) # ry: ignore[RY102]\n})\n",
            None,
        ),
        (
            "f <- function() {\n  1L == 1L # ry: ignore[RY034]\n}\n",
            Some(1),
        ),
    ] {
        let findings = journey.replace(source).await;
        let audits: Vec<_> = findings.iter().filter(|d| d["code"] == "RY113").collect();
        assert_eq!(
            audits.len(),
            usize::from(expected_line.is_some()),
            "{source}: {findings:?}"
        );
        if let Some(line) = expected_line {
            assert_eq!(audits[0]["range"]["start"]["line"], line);
        }
    }
}

/// Closing, disabling, or excluding the document withdraws the actions
/// for the diagnostics it last published.
async fn closed_and_disabled_documents_get_no_actions(journey: &mut Journey) {
    let source = "x <- never_bound_here\n";
    let diagnostic = journey.replace(source).await.remove(0);
    let actions = journey.actions(&diagnostic).await;
    assert_eq!(actions.as_array().unwrap().len(), 2, "{actions}");

    let mark = journey.session.publication_mark();
    journey.session.close(&journey.uri).await.unwrap();
    let closed = journey
        .session
        .published_diagnostics_after(&journey.uri, mark)
        .await
        .unwrap();
    assert!(normalize_diagnostics(&closed).is_empty(), "{closed}");
    let actions = journey.actions(&diagnostic).await;
    assert!(actions.is_null(), "closed: {actions}");

    journey.version += 1;
    let publish =
        harness::open_published(&mut journey.session, &journey.uri, journey.version, source).await;
    let diagnostic = normalize_diagnostics(&publish).remove(0);
    let actions = journey.actions(&diagnostic).await;
    assert_eq!(actions.as_array().unwrap().len(), 2, "{actions}");
    let disabled = journey.configure(json!({"ry": {"enable": false}})).await;
    assert!(disabled.is_empty(), "{disabled:?}");
    let actions = journey.actions(&diagnostic).await;
    assert!(actions.is_null(), "disabled: {actions}");

    let diagnostic = journey
        .configure(json!({"ry": {"enable": true}}))
        .await
        .remove(0);
    journey
        .fixture
        .write_file("ry.toml", "exclude = [\"main.R\"]\n")
        .unwrap();
    let excluded = journey.configure(json!({})).await;
    assert!(excluded.is_empty(), "{excluded:?}");
    let actions = journey.actions(&diagnostic).await;
    assert!(actions.is_null(), "excluded: {actions}");
}
