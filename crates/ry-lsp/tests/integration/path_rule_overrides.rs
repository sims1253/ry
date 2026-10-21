//! Per-file rule severities use the same config-root scope in CLI and LSP,
//! and a watched config edit republishes the new policy without a source edit.

use crate::harness;

use harness::{cli_diagnostics, file_uri, join_session, notify_changed, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

fn severity(publish: &Value, code: &str) -> Option<i64> {
    publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == code)
        .and_then(|finding| finding["severity"].as_i64())
}

#[tokio::test]
async fn path_policy_matches_cli_and_reloads_in_editor() {
    let fixture = FixtureProject::empty().unwrap();
    let initial = "warn = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/**\"]\nerror = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/scratch/**\"]\nwarn = [\"RY040\"]\n";
    fixture.write_file("ry.toml", initial).unwrap();
    let source = "x <- \"a\" + 1L\n";
    fixture.write_file("R/main.R", source).unwrap();
    fixture.write_file("R/scratch/try.R", source).unwrap();

    let cli = cli_diagnostics(fixture.root(), ".", &[]);
    for (path, severity) in [("R/main.R", "error"), ("R/scratch/try.R", "warning")] {
        assert!(
            cli.iter()
                .any(|d| d.path == path && d.code == "RY040" && d.severity == severity),
            "{cli:?}"
        );
    }

    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let main_uri = file_uri(&fixture.path("R/main.R"));
    let scratch_uri = file_uri(&fixture.path("R/scratch/try.R"));
    let main = harness::open_published(&mut session, &main_uri, 1, source).await;
    assert_eq!(severity(&main, "RY040"), Some(1), "{main}");
    let scratch = harness::open_published(&mut session, &scratch_uri, 1, source).await;
    assert_eq!(severity(&scratch, "RY040"), Some(2), "{scratch}");

    let unsaved_uri = tower_lsp::lsp_types::Url::from_file_path(fixture.path("R/unsaved.R"))
        .unwrap()
        .to_string();
    let unsaved = harness::open_published(&mut session, &unsaved_uri, 1, source).await;
    assert_eq!(severity(&unsaved, "RY040"), Some(1), "{unsaved}");

    fixture
        .write_file(
            "ry.toml",
            "warn = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/**\"]\nignore = [\"RY040\"]\n",
        )
        .unwrap();
    let mark = session.publication_mark();
    notify_changed(&mut session, &file_uri(&fixture.path("ry.toml"))).await;
    let changed = session
        .published_diagnostics_after(&main_uri, mark)
        .await
        .unwrap();
    assert_eq!(severity(&changed, "RY040"), None, "{changed}");
    join_session(session, server).await;
}

#[tokio::test]
async fn explicit_editor_rule_choice_wins_over_matching_path_table() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file(
            "ry.toml",
            "[[rule-overrides]]\npaths = [\"R/**\"]\nignore = [\"RY040\"]\n",
        )
        .unwrap();
    let source = "x <- \"a\" + 1L\n";
    fixture.write_file("R/main.R", source).unwrap();
    let (mut session, server) = spawn_session(
        &[fixture.root()],
        json!({}),
        Some(json!({
            "settings": [{"lint": {"error": ["RY040"]}}],
            "globalSettings": {}
        })),
    )
    .await;
    let uri = file_uri(&fixture.path("R/main.R"));
    let publish = harness::open_published(&mut session, &uri, 1, source).await;
    assert_eq!(severity(&publish, "RY040"), Some(1), "{publish}");
    join_session(session, server).await;
}

#[tokio::test]
async fn editor_ignore_list_replacement_survives_unrelated_path_table() {
    for (settings, expected) in [
        (json!({"lint": {"ignore": [], "error": ["RY040"]}}), Some(1)),
        (json!({"lint": {"error": ["RY040"]}}), None),
    ] {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "ignore = [\"RY040\"]\n")
            .unwrap();
        let source = "x <- \"a\" + 1L\n";
        let mut uris = Vec::new();
        for path in ["R/inside.R", "scripts/outside.R"] {
            fixture.write_file(path, source).unwrap();
            uris.push(file_uri(&fixture.path(path)));
        }
        let (mut session, server) = spawn_session(
            &[fixture.root()],
            json!({}),
            Some(json!({"settings": [settings], "globalSettings": {}})),
        )
        .await;
        for uri in &uris {
            let publish = harness::open_published(&mut session, uri, 1, source).await;
            assert_eq!(severity(&publish, "RY040"), expected, "{publish}");
        }
        harness::sync_barrier(&mut session, &uris[0]).await;

        fixture
            .write_file(
                "ry.toml",
                "ignore = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/**\"]\nwarn = [\"RY010\"]\n",
            )
            .unwrap();
        let mark = session.publication_mark();
        notify_changed(&mut session, &file_uri(&fixture.path("ry.toml"))).await;
        for uri in &uris {
            let publish = session
                .published_diagnostics_after(uri, mark)
                .await
                .unwrap();
            assert_eq!(severity(&publish, "RY040"), expected, "{publish}");
        }
        join_session(session, server).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_workspace_config_root_keeps_scoped_editor_severity() {
    use std::os::unix::fs::symlink;

    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file(
            "ry.toml",
            "warn = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/**\"]\nerror = [\"RY040\"]\n",
        )
        .unwrap();
    let source = "x <- \"a\" + 1L\n";
    fixture.write_file("R/a.R", source).unwrap();
    let aliases = FixtureProject::empty().unwrap();
    let linked_root = aliases.path("linked-root");
    symlink(fixture.root(), &linked_root).unwrap();

    let (mut session, server) = spawn_session(&[&linked_root], json!({}), None).await;
    let uri = file_uri(&linked_root.join("R/a.R"));
    let publish = harness::open_published(&mut session, &uri, 1, source).await;
    assert_eq!(severity(&publish, "RY040"), Some(1), "{publish}");
    join_session(session, server).await;
}
