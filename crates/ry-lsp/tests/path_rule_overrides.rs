//! Per-file rule severities use the same config-root scope in CLI and LSP,
//! and a watched config edit republishes the new policy without a source edit.

mod harness;

use harness::{join_session, spawn_session};
use ry_testkit::{CliProcess, FixtureProject, file_uri};
use serde_json::{Value, json};

fn run<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn severity(publish: &Value, code: &str) -> Option<i64> {
    publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == code)
        .and_then(|finding| finding["severity"].as_i64())
}

#[test]
fn path_policy_matches_cli_and_reloads_in_editor() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        let initial = "warn = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/**\"]\nerror = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/scratch/**\"]\nwarn = [\"RY040\"]\n";
        fixture.write_file("ry.toml", initial).unwrap();
        let source = "x <- \"a\" + 1L\n";
        fixture.write_file("R/main.R", source).unwrap();
        fixture.write_file("R/scratch/try.R", source).unwrap();

        let cli = CliProcess::new(harness::ry_binary())
            .check(&fixture, ".", ["--output-format", "json"])
            .unwrap();
        let cli: Vec<Value> = serde_json::from_slice(&cli.stdout).unwrap();
        assert!(
            cli.iter().any(
                |d| d["path"].as_str().is_some_and(|p| p.ends_with("R/main.R"))
                    && d["code"] == "RY040"
                    && d["severity"] == "error"
            ),
            "{cli:?}"
        );
        assert!(
            cli.iter().any(|d| d["path"]
                .as_str()
                .is_some_and(|p| p.ends_with("R/scratch/try.R"))
                && d["code"] == "RY040"
                && d["severity"] == "warning"),
            "{cli:?}"
        );

        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        let main_uri = file_uri(&fixture.path("R/main.R")).unwrap();
        let scratch_uri = file_uri(&fixture.path("R/scratch/try.R")).unwrap();
        let mark = session.publication_mark();
        session.open(&main_uri, 1, source).await.unwrap();
        let main = session
            .published_diagnostics_after(&main_uri, mark)
            .await
            .unwrap();
        assert_eq!(severity(&main, "RY040"), Some(1), "{main}");
        let mark = session.publication_mark();
        session.open(&scratch_uri, 1, source).await.unwrap();
        let scratch = session
            .published_diagnostics_after(&scratch_uri, mark)
            .await
            .unwrap();
        assert_eq!(severity(&scratch, "RY040"), Some(2), "{scratch}");

        fixture
            .write_file(
                "ry.toml",
                "warn = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/**\"]\nignore = [\"RY040\"]\n",
            )
            .unwrap();
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": file_uri(&fixture.path("ry.toml")).unwrap(), "type": 2}]}),
            )
            .await
            .unwrap();
        let changed = session
            .published_diagnostics_after(&main_uri, mark)
            .await
            .unwrap();
        assert_eq!(severity(&changed, "RY040"), None, "{changed}");
        join_session(session, server).await;
    })
}

#[test]
fn explicit_editor_rule_choice_wins_over_matching_path_table() {
    run(async {
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
        let uri = file_uri(&fixture.path("R/main.R")).unwrap();
        let mark = session.publication_mark();
        session.open(&uri, 1, source).await.unwrap();
        let publish = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert_eq!(severity(&publish, "RY040"), Some(1), "{publish}");
        join_session(session, server).await;
    })
}
