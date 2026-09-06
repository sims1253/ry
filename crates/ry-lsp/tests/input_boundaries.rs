mod harness;

use harness::{
    file_uri, join_session, normalize_diagnostics, ry_binary, spawn_session, sync_barrier,
};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};
use std::process::Command;

fn cli(fixture: &FixtureProject, target: &str) -> Vec<Value> {
    let output = Command::new(ry_binary())
        .current_dir(fixture.root())
        .args(["check", "--output-format", "json", target])
        .output()
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn latin1_sibling_survives_open_and_close() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            let source = "value <- make_label() + 1\n";
            fixture.write_file("main.R", source).unwrap();
            fixture
                .write_file("helper.R", b"make_label <- function() \"caf\xe9\"\n")
                .unwrap();
            assert!(cli(&fixture, ".").iter().any(|d| d["code"] == "RY040"));
            let uri = file_uri(&fixture.path("main.R"));
            let helper_uri = file_uri(&fixture.path("helper.R"));
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
            let mark = session.publication_mark();
            session.open(&uri, 1, source).await.unwrap();
            let first = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            let expected = normalize_diagnostics(&first);
            assert!(expected.iter().any(|d| d["code"] == "RY040"), "{first}");
            for (version, open) in [(2, true), (3, false)] {
                if open {
                    session
                        .open(&helper_uri, 1, "make_label <- function() \"café\"\n")
                        .await
                        .unwrap();
                } else {
                    session
                        .notify(
                            "textDocument/didClose",
                            json!({"textDocument":{"uri":helper_uri}}),
                        )
                        .await
                        .unwrap();
                }
                sync_barrier(&mut session, &uri).await;
                let mark = session.publication_mark();
                session
                    .change(&uri, version, json!([{"text":source}]))
                    .await
                    .unwrap();
                let publish = session
                    .published_diagnostics_after(&uri, mark)
                    .await
                    .unwrap();
                assert_eq!(normalize_diagnostics(&publish), expected);
            }
            join_session(session, server).await;
        });
}

#[test]
fn environment_paths_are_anchored_globs_in_both_frontends() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", "[[environments]]\nname = 'shiny'\nbindings = ['profile_binding']\npaths = ['pkg/inst/shiny/**', 'pkg/exact.R']\n").unwrap();
    fixture
        .write_file("pkg/DESCRIPTION", "Package: example\nVersion: 0.1\n")
        .unwrap();
    let paths = [
        "pkg/inst/shiny/main.R",
        "pkg/inst/shiny/nested/main.R",
        "pkg/exact.R",
        "pkg/inst/shiny-unrelated/main.R",
        "ancestor/pkg/inst/shiny/main.R",
    ];
    for path in paths {
        fixture.write_file(path, "profile_binding\n").unwrap();
    }
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    for (index, path) in paths.iter().enumerate() {
        let expected_unbound = index >= 3;
        let findings = cli(&fixture, path);
        assert_eq!(
            findings.iter().any(|d| d["code"] == "RY010"),
            expected_unbound,
            "{path}: {findings:?}"
        );
        let uri = file_uri(&fixture.path(path));
        let mark = session.publication_mark();
        session.open(&uri, 1, "profile_binding\n").await.unwrap();
        let publish = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert_eq!(
            normalize_diagnostics(&publish)
                .iter()
                .any(|d| d["code"] == "RY010"),
            expected_unbound,
            "{path}: {publish}"
        );
    }
    join_session(session, server).await;
    });
}
