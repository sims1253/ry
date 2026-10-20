//! The editor publishes what `ry check` reports for the same file.

use ry_testkit::{FixtureProject, JsonRpcProcess};
use serde_json::{Value, json};
use std::process::Command;

use crate::harness;

use harness::{Published, cli_diagnostics, file_uri, published_from_lsp, ry_binary};

/// Open `target` in a fresh server initialized with one folder's settings
/// and return its first publication.
async fn lsp_diagnostics(
    fixture: &FixtureProject,
    target: &str,
    initialization_settings: Value,
) -> Vec<Published> {
    let (mut session, server) = harness::spawn_session(
        &[fixture.root()],
        json!({}),
        Some(json!({
            "settings": [initialization_settings],
            "globalSettings": {}
        })),
    )
    .await;
    let path = fixture.path(target);
    let text = std::fs::read_to_string(&path).unwrap();
    let publish = harness::open_published(&mut session, &file_uri(&path), 1, &text).await;
    harness::join_session(session, server).await;
    published_from_lsp(&publish, &path, fixture.root())
}

fn settings(fixture: &FixtureProject) -> Value {
    let path = fixture.path("lsp-settings.json");
    if path.is_file() {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    } else {
        json!({})
    }
}

/// What a matrix row checks beyond CLI/LSP equality.
#[derive(Clone, Copy)]
enum Expect {
    Parity,
    /// Neither mode reports anything for the file.
    Quiet,
    /// No RY010 names this binding.
    Bound(&'static str),
}

#[tokio::test]
async fn cli_and_run_with_publish_the_same_single_root_matrix() {
    let inline = |files: &[(&str, &str)]| {
        let fixture = FixtureProject::empty().unwrap();
        for (path, text) in files {
            fixture.write_file(path, text).unwrap();
        }
        fixture
    };
    let fixture = |name: &str| FixtureProject::from_fixture(name).unwrap();
    let high = &["--min-confidence", "high"][..];
    // The baseline rows put identical findings on both sides of
    // suppression and the confidence threshold: the baseline count must be
    // consumed after both, in both modes.
    let cases = [
        (
            fixture("filtering/ignore"),
            "diagnostic.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("filtering/select"),
            "diagnostic.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("filtering/extend-select"),
            "diagnostic.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("filtering/error"),
            "diagnostic.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("filtering/warn"),
            "diagnostic.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("filtering/exclude"),
            "diagnostic.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("filtering/baseline"),
            "diagnostic.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("filtering/baseline-suppression"),
            "diagnostic.R",
            &[][..],
            Expect::Quiet,
        ),
        (
            fixture("filtering/baseline-min-confidence"),
            "diagnostic.R",
            high,
            Expect::Quiet,
        ),
        (
            fixture("filtering/min-confidence"),
            "diagnostic.R",
            high,
            Expect::Parity,
        ),
        (fixture("unicode"), "R/non_ascii.R", &[][..], Expect::Parity),
        (
            fixture("complete-package"),
            "R/imports.R",
            &[][..],
            Expect::Bound("imported_helper"),
        ),
        (
            fixture("complete-package"),
            "R/native.R",
            &[][..],
            Expect::Parity,
        ),
        (
            fixture("excluded-influence"),
            "kept.R",
            &[][..],
            Expect::Parity,
        ),
        // Rules that are off by default.
        (
            inline(&[("diagnostic.R", "if (1L) print(1)\n")]),
            "diagnostic.R",
            &[][..],
            Expect::Quiet,
        ),
        // An explicit empty `select` turns off the default rules.
        (
            inline(&[
                ("ry.toml", "select = []\n"),
                ("diagnostic.R", "x <- missing_name\n"),
            ]),
            "diagnostic.R",
            &[][..],
            Expect::Quiet,
        ),
    ];
    for (index, (fixture, target, cli_args, expect)) in cases.into_iter().enumerate() {
        let cli = cli_diagnostics(fixture.root(), fixture.root(), cli_args)
            .into_iter()
            .filter(|d| d.path == target)
            .collect::<Vec<_>>();
        let lsp = lsp_diagnostics(&fixture, target, settings(&fixture)).await;
        assert_eq!(
            lsp, cli,
            "row {index} ({target}): published diagnostics differ"
        );
        match expect {
            Expect::Parity => {}
            Expect::Quiet => assert!(cli.is_empty(), "row {index}: {cli:?}"),
            Expect::Bound(name) => assert!(
                !cli.iter()
                    .any(|d| d.code == "RY010" && d.message.contains(name)),
                "row {index}: {cli:?}"
            ),
        }
    }
}

/// Support-tree findings drop one confidence tier before the threshold in
/// both modes. `non-source-demotion` is a package whose `tests/`,
/// `data-raw/`, `demo/`, `vignettes/`, and `inst/` files (and a nested
/// package's `tests/`) carry the same medium-confidence RY010 as the `R/`
/// controls. `non-source-plain-tests` has a `tests/` directory with no
/// `DESCRIPTION` above it, so nothing is demoted.
#[tokio::test]
async fn non_source_trees_demote_before_the_threshold_in_both_modes() {
    let cases = [
        ("filtering/non-source-demotion", "R/control.R", true),
        ("filtering/non-source-demotion", "subpkg/R/control.R", true),
        (
            "filtering/non-source-demotion",
            "tests/testthat/helper.R",
            false,
        ),
        ("filtering/non-source-demotion", "data-raw/gen.R", false),
        ("filtering/non-source-demotion", "demo/demo.R", false),
        ("filtering/non-source-demotion", "vignettes/vig.R", false),
        ("filtering/non-source-demotion", "inst/doc.R", false),
        (
            "filtering/non-source-demotion",
            "subpkg/tests/helper.R",
            false,
        ),
        ("filtering/non-source-plain-tests", "tests/example.R", true),
    ];
    for (fixture_name, target, visible) in cases {
        let fixture = FixtureProject::from_fixture(fixture_name).unwrap();
        let cli = cli_diagnostics(
            fixture.root(),
            fixture.root(),
            &["--min-confidence", "medium"],
        )
        .into_iter()
        .filter(|d| d.path == target)
        .collect::<Vec<_>>();
        assert_eq!(
            !cli.is_empty(),
            visible,
            "{fixture_name}/{target}: `ry check` visibility is wrong"
        );
        let lsp = lsp_diagnostics(&fixture, target, settings(&fixture)).await;
        assert_eq!(
            lsp, cli,
            "{fixture_name}/{target}: the editor must demote exactly when `ry check` does"
        );
    }
}

/// Diagnostics for indexed files that were never opened take their UTF-16
/// ranges from the checked source.
#[tokio::test]
async fn indexed_unopened_diagnostics_use_the_checked_source_for_utf16_ranges() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("disk.R", "emoji <- \"😀\"; length(xx = 1L)\r\n")
        .unwrap();
    fixture.write_file("trigger.R", "ok <- 1L\n").unwrap();
    let (mut session, server) = harness::spawn_session(&[fixture.root()], json!({}), None).await;
    let disk_uri = file_uri(&fixture.path("disk.R"));
    let mark = session.publication_mark();
    session
        .open(&file_uri(&fixture.path("trigger.R")), 1, "ok <- 1L\n")
        .await
        .unwrap();
    let publication = session
        .published_diagnostics_after(&disk_uri, mark)
        .await
        .unwrap();
    harness::join_session(session, server).await;

    let diagnostics = publication["params"]["diagnostics"].as_array().unwrap();
    let range = |code: &str| {
        diagnostics
            .iter()
            .find(|diagnostic| diagnostic["code"] == code)
            .unwrap_or_else(|| panic!("missing {code} in {diagnostics:?}"))["range"]
            .clone()
    };
    assert_eq!(
        range("RY090"),
        json!({
            "start": {"line": 0, "character": 22},
            "end": {"line": 0, "character": 29}
        })
    );
    assert_eq!(
        range("RY091"),
        json!({
            "start": {"line": 0, "character": 15},
            "end": {"line": 0, "character": 30}
        })
    );
}

/// The real `ry server` process frames a full request, notification,
/// response, and exit exchange with correct Content-Length headers and no
/// stray stdout output.
#[test]
fn cross_mode_subprocess_framing_survives_multi_round_exchange() {
    let fixture = FixtureProject::from_fixture("shared").unwrap();
    let mut command = Command::new(ry_binary());
    command.arg("server").current_dir(fixture.root());
    let mut client = JsonRpcProcess::spawn(&mut command).unwrap();
    let root_uri = file_uri(fixture.root());

    // Round 1: initialize request → response.
    let init_id = client
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "initializationOptions": {
                    "settings": [{}],
                    "globalSettings": {}
                },
            }),
        )
        .unwrap();
    let init_response = client
        .receive_until(|m| m.get("id") == Some(&json!(init_id)), 8)
        .unwrap();
    assert_eq!(
        init_response.pointer("/result/serverInfo/name"),
        Some(&json!("ry")),
    );

    // Round 2: initialized notification (no response expected), then
    // didOpen → publishDiagnostics notification.
    client.notify("initialized", json!({})).unwrap();
    let path = fixture.path("R/diagnostic.R");
    let uri = file_uri(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    client
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "r", "version": 1, "text": text
            }}),
        )
        .unwrap();
    let publish = client
        .receive_until(
            |m| {
                m.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                    && m.pointer("/params/uri") == Some(&json!(uri))
            },
            64,
        )
        .unwrap();
    // The diagnostics array must be present and well-formed (framing
    // correctness implies the full array survived intact).
    assert!(
        publish.pointer("/params/diagnostics").is_some(),
        "publishDiagnostics notification missing diagnostics array",
    );

    // Round 3: shutdown request → response, then exit notification.
    let shutdown_id = client.request("shutdown", Value::Null).unwrap();
    let shutdown_response = client
        .receive_until(|m| m.get("id") == Some(&json!(shutdown_id)), 8)
        .unwrap();
    assert_eq!(shutdown_response["result"], Value::Null);
    client.notify("exit", Value::Null).unwrap();
}
