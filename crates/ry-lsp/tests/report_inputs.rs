//! The editor checks the original report coordinates through the shared mask.
mod harness;

use harness::{ClientSession, join_session, spawn_session, sync_barrier};
use ry_testkit::{FixtureProject, file_uri};
use serde_json::{Value, json};

const ENABLED: &str = "[reports]\nenabled = true\n";
const DISABLED: &str = "[reports]\nenabled = false\n";

fn finding<'a>(publish: &'a Value, code: &str) -> Option<&'a Value> {
    publish["params"]["diagnostics"]
        .as_array()?
        .iter()
        .find(|diag| diag["code"] == code)
}

/// `None` for a null response, else whether any hint was returned.
async fn hints(session: &mut ClientSession, uri: &str) -> Option<bool> {
    let hints = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 4, "character": 0}}
            }),
        )
        .await
        .unwrap();
    (!hints.is_null()).then(|| hints.as_array().is_some_and(|hints| !hints.is_empty()))
}

/// Rewrite `ry.toml`, announce it, and return `uri`'s next publication.
async fn write_config(
    session: &mut ClientSession,
    fixture: &FixtureProject,
    uri: &str,
    config: &str,
) -> Value {
    fixture.write_file("ry.toml", config).unwrap();
    sync_barrier(session, uri).await;
    let mark = session.publication_mark();
    let config_uri = file_uri(&fixture.path("ry.toml")).unwrap();
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": config_uri, "type": 2}]}),
        )
        .await
        .unwrap();
    session
        .published_diagnostics_after(uri, mark)
        .await
        .unwrap()
}

/// Replace `uri`'s text (or apply `changes`) and return its next publication.
async fn change(session: &mut ClientSession, uri: &str, version: i32, changes: Value) -> Value {
    let mark = session.publication_mark();
    session.change(uri, version, changes).await.unwrap();
    session
        .published_diagnostics_after(uri, mark)
        .await
        .unwrap()
}

async fn replace(session: &mut ClientSession, uri: &str, version: i32, text: &str) -> Value {
    change(session, uri, version, json!([{"text": text}])).await
}

async fn open(session: &mut ClientSession, uri: &str, text: &str) -> Value {
    let mark = session.publication_mark();
    session.open(uri, 1, text).await.unwrap();
    session
        .published_diagnostics_after(uri, mark)
        .await
        .unwrap()
}

#[tokio::test]
async fn disabled_reports_never_supply_inlay_hints_even_after_config_toggles() {
    let fixture = FixtureProject::empty().unwrap();
    let outside = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", DISABLED).unwrap();
    let source = "```{r}\nx <- 1L\n```\n";
    let report_uri = file_uri(&fixture.write_file("memo.qmd", source).unwrap()).unwrap();
    let outside_uri = file_uri(&outside.write_file("outside.qmd", source).unwrap()).unwrap();
    let ordinary_uri = file_uri(&fixture.write_file("ordinary.R", "x <- 1L\n").unwrap()).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    session.open(&report_uri, 1, source).await.unwrap();
    session.open(&outside_uri, 1, source).await.unwrap();
    session.open(&ordinary_uri, 1, "x <- 1L\n").await.unwrap();
    for (config, reports) in [
        (None, false),
        (Some(ENABLED), true),
        (Some(DISABLED), false),
    ] {
        if let Some(config) = config {
            write_config(&mut session, &fixture, &report_uri, config).await;
        }
        let expected = reports.then_some(true);
        assert_eq!(hints(&mut session, &report_uri).await, expected);
        assert_eq!(hints(&mut session, &outside_uri).await, expected);
        assert_eq!(hints(&mut session, &ordinary_uri).await, Some(true));
    }
    join_session(session, server).await;
}

#[tokio::test]
async fn report_config_toggle_rechecks_an_open_buffer() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", DISABLED).unwrap();
    let source = "```{r}\n\"a\" + 1L\n```\n";
    let uri = file_uri(&fixture.write_file("memo.qmd", source).unwrap()).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let initial = open(&mut session, &uri, source).await;
    assert!(finding(&initial, "RY040").is_none(), "{initial}");
    let enabled = write_config(&mut session, &fixture, &uri, ENABLED).await;
    assert!(finding(&enabled, "RY040").is_some(), "{enabled}");
    let disabled = write_config(&mut session, &fixture, &uri, DISABLED).await;
    assert!(finding(&disabled, "RY040").is_none(), "{disabled}");
    join_session(session, server).await;
}

#[tokio::test]
async fn warm_report_execution_identity_edits_republish_the_same_uri() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ENABLED).unwrap();
    let source = "😀 prose\r\n```{r, Eval=FALSE}\r\n'a' + 1L\r\n```\r\n";
    let uri = file_uri(&fixture.write_file("memo.qmd", source).unwrap()).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let active = open(&mut session, &uri, source).await;
    assert!(finding(&active, "RY040").is_some(), "{active}");

    // Each edit either runs `'a' + 1L` (RY040), stops at RY121, or neither.
    let later = "```{r}\r\n'a' + 1L\r\n```\r\n";
    let body = |body: &str| format!("😀 prose\r\n```{{r}}\r\n{body}\r\n```\r\n{later}");
    let yaml = |yaml: &str| format!("---\r\n{yaml}\r\n---\r\n{later}");
    let cases = [
        (
            "😀 prose\r\n```{r, eval=FALSE}\r\n'a' + 1L\r\n```\r\n".to_owned(),
            None,
        ),
        (
            body(r"knitr::`opts_\x63hunk`$set(eval=FALSE)"),
            Some("RY121"),
        ),
        (
            body(r#"knitr::"opts_\x63hunk"$set(eval=FALSE)"#),
            Some("RY121"),
        ),
        (
            yaml("format: {html: {execute: {eval: false}}}"),
            Some("RY121"),
        ),
        (
            yaml("{format: {html: {execute: {eval: false}}}}"),
            Some("RY121"),
        ),
        (
            yaml("  {format: {html: {execute: {eval: false}}}}"),
            Some("RY121"),
        ),
        (yaml("metadata: {eval: false}"), Some("RY040")),
        (
            yaml("{title: \"test\", metadata: {execute: {eval: false}}}"),
            Some("RY040"),
        ),
    ];
    for (version, (source, expected)) in (2..).zip(cases) {
        let published = replace(&mut session, &uri, version, &source).await;
        for code in ["RY040", "RY121"] {
            let found = finding(&published, code).is_some();
            assert_eq!(found, expected == Some(code), "{code}: {published}");
        }
    }
    join_session(session, server).await;
}

#[tokio::test]
async fn warm_report_prose_and_option_edits_keep_original_utf16_positions() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ENABLED).unwrap();
    let report = |prose: &str, eval: &str| {
        format!(
            "{prose}\r\n```{{r}}\r\nx <- \"a\"\r\n```\r\n```{{r}}\r\n#| eval: {eval}\r\nx + 1L\r\n```\r\n"
        )
    };
    let start = |publish: &Value, code: &str| {
        let start = &finding(publish, code).unwrap()["range"]["start"];
        (
            start["line"].as_u64().unwrap(),
            start["character"].as_u64().unwrap(),
        )
    };
    let first = report("😀 prose", "true");
    let uri = file_uri(&fixture.write_file("memo.qmd", &first).unwrap()).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let published = open(&mut session, &uri, &first).await;
    assert_eq!(start(&published, "RY040"), (6, 0), "{published}");
    let bad = finding(&published, "RY040").unwrap();
    let action = session
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": uri},
                "range": bad["range"],
                "context": {"diagnostics": [bad], "only": ["quickfix"]},
            }),
        )
        .await
        .unwrap();
    assert!(action.is_null(), "report edits are refused: {action}");

    // A prose-only UTF-16 range edit changes two surrogate units but
    // leaves the R chunk at its original line and byte coordinates.
    let insert = json!([{"range": {
        "start": {"line": 0, "character": 0},
        "end": {"line": 0, "character": 0}
    }, "text": "界"}]);
    let prose = change(&mut session, &uri, 2, insert).await;
    assert_eq!(start(&prose, "RY040"), (6, 0), "{prose}");
    let disabled = replace(&mut session, &uri, 3, &report("界 prose", "false")).await;
    assert!(finding(&disabled, "RY040").is_none(), "{disabled}");
    let moved = replace(
        &mut session,
        &uri,
        4,
        &report("界 prose\r\nextra prose", "true"),
    )
    .await;
    assert_eq!(start(&moved, "RY040"), (7, 0), "{moved}");
    let broken = replace(
        &mut session,
        &uri,
        5,
        "界 prose\r\n```{r}\r\nx <- (\r\n```\r\n",
    )
    .await;
    assert_eq!(start(&broken, "RY000"), (2, 2), "{broken}");
    assert!(finding(&broken, "RY040").is_none(), "{broken}");
    join_session(session, server).await;
}

/// A range edit that disables a chunk masks text outside the edited range.
/// Reusing the previous masked tree would keep that chunk's stale nodes.
#[tokio::test]
async fn range_edit_that_disables_a_chunk_reparses_the_whole_report() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ENABLED).unwrap();
    let source = "```{r, eval=TRUE}\nf <- function(x) x\n\"a\" + 1L\n```\n";
    let uri = file_uri(&fixture.write_file("memo.qmd", source).unwrap()).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let active = open(&mut session, &uri, source).await;
    assert!(finding(&active, "RY040").is_some(), "{active}");
    let edit = json!([{"range": {
        "start": {"line": 0, "character": 12},
        "end": {"line": 0, "character": 16}
    }, "text": "FALSE"}]);
    let disabled = change(&mut session, &uri, 2, edit).await;
    assert_eq!(disabled["params"]["diagnostics"], json!([]), "{disabled}");
    join_session(session, server).await;
}

#[tokio::test]
async fn oversized_report_refusal_does_not_reuse_its_empty_parse_tree_after_shrink() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file(
            "ry.toml",
            format!("{ENABLED}[index]\nmax-file-bytes = 3145728\n"),
        )
        .unwrap();
    let huge = "é".repeat(ry_workspace::reports::MAX_REPORT_BYTES / 2 + 1);
    let uri = file_uri(&fixture.write_file("memo.qmd", &huge).unwrap()).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let capped = open(&mut session, &uri, &huge).await;
    assert!(finding(&capped, "RY120").is_some(), "{capped}");
    let admitted = replace(&mut session, &uri, 2, "```{r}\n\"x\" + 1L\n```\n").await;
    assert!(finding(&admitted, "RY120").is_none(), "{admitted}");
    assert!(finding(&admitted, "RY040").is_some(), "{admitted}");
    join_session(session, server).await;
}

#[tokio::test]
async fn two_open_reports_keep_independent_r_bindings() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ENABLED).unwrap();
    let first = "```{r}\nx <- 1L\n```\n";
    let second = "```{r}\nx + 1L\n```\n";
    let first_uri = file_uri(&fixture.write_file("first.qmd", first).unwrap()).unwrap();
    let second_uri = file_uri(&fixture.write_file("second.qmd", second).unwrap()).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    session.open(&first_uri, 1, first).await.unwrap();
    let published = open(&mut session, &second_uri, second).await;
    assert!(finding(&published, "RY010").is_some(), "{published}");
    join_session(session, server).await;
}
