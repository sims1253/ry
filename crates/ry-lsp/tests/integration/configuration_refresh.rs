//! Configuration changes reach the next publication without a file edit
//! or restart, and `enable: false` turns off diagnostics and inlay hints
//! for its folder.

use ry_lsp::test_seam::Seam;
use ry_testkit::FixtureProject;
use serde_json::json;

use crate::harness;

use harness::{count_code, file_uri, join_session, notify_changed, spawn_session};

// ════════════════════════════════════════════════════════════════════════════
// didChangeConfiguration refreshes the cached filter values
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn did_change_configuration_refreshes_cached_filters() {
    let fixture = FixtureProject::empty().unwrap();
    // RY090 (partial argument name) is emitted at Medium confidence by
    // default, so raising `minConfidence` to "high" suppresses it.
    fixture
        .write_file("R/diag.R", "z <- length(xx = 1L)\n")
        .unwrap();

    let (mut session, _server) = spawn_session(&[fixture.root()], json!({}), None).await;

    let diag_uri = file_uri(&fixture.path("R/diag.R"));

    // Open and capture the initial publication: RY090 must be present.
    let initial =
        harness::open_published(&mut session, &diag_uri, 1, "z <- length(xx = 1L)\n").await;
    assert!(
        count_code(&initial, "RY090") >= 1,
        "RY090 should be present at the default confidence; got: {:?}",
        initial["params"]["diagnostics"]
    );

    // Raise minConfidence via workspace/didChangeConfiguration — no file
    // edit, no restart. Mark after the initial publication is consumed so
    // the next diag.R publish is the one triggered by this notification.
    let mark1 = session.publication_mark();
    session
        .notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": { "ry": { "minConfidence": "high" } } }),
        )
        .await
        .unwrap();
    let after = session
        .published_diagnostics_after(&diag_uri, mark1)
        .await
        .unwrap();

    // The recomputed min_confidence must suppress RY090.
    assert_eq!(
        count_code(&after, "RY090"),
        0,
        "RY090 should be suppressed after raising minConfidence; got: {:?}",
        after["params"]["diagnostics"]
    );
}

// ════════════════════════════════════════════════════════════════════════════
// PR #79 round 3: the pull-based counterpart. A client advertising
// `workspace.configuration = true` is answered via `workspace/configuration`
// requests; the per-folder settings pulled that way must reach each folder
// context and refresh its cached values.
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn did_change_configuration_pull_applies_per_folder_settings() {
    let fixture = FixtureProject::empty().unwrap();
    // Same RY090/min-confidence setup as the test above.
    fixture
        .write_file("R/diag.R", "z <- length(xx = 1L)\n")
        .unwrap();

    let (mut session, _server) = spawn_session(
        &[fixture.root()],
        json!({ "workspace": { "configuration": true } }),
        None,
    )
    .await;
    // Answer the initial `workspace/configuration` pull during
    // `initialized` with default settings (one per folder root, then a
    // root-scoped item).
    session
        .respond_to_request("workspace/configuration", json!([{}, {}]))
        .await
        .unwrap();

    let diag_uri = file_uri(&fixture.path("R/diag.R"));

    // Open and capture the initial publication: RY090 must be present at
    // the default confidence (the initial pull returned empty settings).
    let initial =
        harness::open_published(&mut session, &diag_uri, 1, "z <- length(xx = 1L)\n").await;
    assert!(
        count_code(&initial, "RY090") >= 1,
        "RY090 should be present at the default confidence; got: {:?}",
        initial["params"]["diagnostics"]
    );

    // Trigger the pull-based refresh: `workspace/didChangeConfiguration`
    // with no inline settings (the server pulls instead). Answer the
    // server's `workspace/configuration` request with settings raising
    // minConfidence — one per folder root, then a root-scoped item.
    let mark1 = session.publication_mark();
    session
        .notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": {} }),
        )
        .await
        .unwrap();
    session
        .respond_to_request(
            "workspace/configuration",
            json!([
                { "minConfidence": "high" },
                { "minConfidence": "high" },
            ]),
        )
        .await
        .unwrap();
    let after = session
        .published_diagnostics_after(&diag_uri, mark1)
        .await
        .unwrap();

    // The pull must reach the owning folder context and refresh its cached
    // min_confidence, suppressing RY090 without a file edit or restart.
    assert_eq!(
        count_code(&after, "RY090"),
        0,
        "RY090 should be suppressed after the pull raises minConfidence; got: {:?}",
        after["params"]["diagnostics"]
    );
}

// ════════════════════════════════════════════════════════════════════════════
// `enable: false` skips analysis and publishing for the folder
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn enable_false_skips_diagnostics_for_the_folder() {
    let fixture = FixtureProject::empty().unwrap();
    // RY090 would fire at the default confidence when enabled.
    fixture
        .write_file("R/diag.R", "z <- length(xx = 1L)\n")
        .unwrap();

    let (mut session, server) = spawn_session(
        &[fixture.root()],
        json!({}),
        Some(json!({
            "settings": [{"enable": false}],
            "globalSettings": {}
        })),
    )
    .await;

    let diag_uri = file_uri(&fixture.path("R/diag.R"));
    let publish =
        harness::open_published(&mut session, &diag_uri, 1, "z <- length(xx = 1L)\n").await;

    let count = publish["params"]["diagnostics"]
        .as_array()
        .map(|diags| diags.len())
        .unwrap_or(0);
    assert_eq!(
        count, 0,
        "enable: false must publish an empty diagnostics set; got: {publish}"
    );
    join_session(session, server).await;
}

#[tokio::test]
async fn enable_false_skips_inlay_hints_for_the_folder() {
    let fixture = FixtureProject::empty().unwrap();
    // `x <- 1L` yields one integer hint when the folder is enabled
    // (pinned by `inlay_hint_does_not_cross_workspace_roots`).
    fixture.write_file("R/hint.R", "x <- 1L\n").unwrap();

    let (mut session, server) = spawn_session(
        &[fixture.root()],
        json!({}),
        Some(json!({
            "settings": [{"enable": false}],
            "globalSettings": {}
        })),
    )
    .await;

    let hint_uri = file_uri(&fixture.path("R/hint.R"));
    session.open(&hint_uri, 1, "x <- 1L\n").await.unwrap();

    let result = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": hint_uri},
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 1, "character": 0}
                }
            }),
        )
        .await
        .unwrap();

    assert_eq!(
        result,
        serde_json::Value::Null,
        "enable: false must return null instead of inlay hints; got: {result}"
    );
    join_session(session, server).await;
}

#[tokio::test]
async fn config_reload_retains_valid_settings_on_failure() {
    for mode in ["discovered", "relative", "absolute", "ancestor"] {
        let fixture = FixtureProject::empty().unwrap();
        let config_name = match mode {
            "discovered" => "ry.toml",
            "ancestor" => "nested/ry.toml",
            _ => "config/ry.toml",
        };
        let workspace_root = fixture.path(if mode == "ancestor" { "nested" } else { "" });
        fixture
            .write_file(
                "ry.toml",
                if mode == "ancestor" {
                    "error = ['RY002']\n"
                } else {
                    ""
                },
            )
            .unwrap();
        fixture
            .write_file(config_name, "ignore = ['RY002']\n")
            .unwrap();
        let config_uri = file_uri(&fixture.path(config_name));
        let source = "if (c(TRUE, FALSE)) print(1)\n";
        let source_name = if mode == "ancestor" {
            "nested/main.R"
        } else {
            "main.R"
        };
        fixture.write_file(source_name, source).unwrap();
        let options = match mode {
            "relative" => Some(json!({"settings": [{"configuration": config_name}]})),
            "absolute" => Some(json!({"settings": [{"configuration": fixture.path(config_name)}]})),
            _ => None,
        };
        let (mut session, server) =
            spawn_session(&[&workspace_root], json!({}), options.clone()).await;
        let uri = file_uri(&fixture.path(source_name));
        let initial = harness::open_published(&mut session, &uri, 1, source).await;
        assert_eq!(count_code(&initial, "RY002"), 0, "{mode}: {initial}");

        let mut latest = initial;
        // A missing discovered config restores ancestor settings or defaults.
        // A missing explicit override is a load failure and retains its settings.
        for (content, expected) in [
            (Some("ignore = ["), 0),
            (None, usize::from(matches!(mode, "discovered" | "ancestor"))),
            (Some("ignore = ['RY002']\n"), 0),
            (Some(""), 1),
        ] {
            match content {
                Some(content) => {
                    fixture.write_file(config_name, content).unwrap();
                }
                None => std::fs::remove_file(fixture.path(config_name)).unwrap(),
            }
            harness::sync_barrier(&mut session, &uri).await;
            let mark = session.publication_mark();
            match content {
                Some(_) => notify_changed(&mut session, &config_uri).await,
                None => harness::notify_deleted(&mut session, &config_uri).await,
            }
            let after = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            assert_eq!(
                count_code(&after, "RY002"),
                expected,
                "{mode}, {content:?}: {after}"
            );
            if mode == "ancestor" && content.is_none() {
                // The ancestor promotes RY002 to an error; defaults only warn.
                let diagnostic = after["params"]["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|diagnostic| diagnostic["code"] == "RY002")
                    .unwrap();
                assert_eq!(
                    diagnostic["severity"], 1,
                    "ancestor config must apply: {after}"
                );
            }
            latest = after;
        }
        harness::join_session(session, server).await;

        // A cold server has no previous config to retain. Failed explicit
        // overrides use defaults, even if a different ry.toml is valid.
        for content in ["", "ignore = ["] {
            fixture
                .write_file("ry.toml", "ignore = ['RY002']\n")
                .unwrap();
            fixture.write_file(config_name, content).unwrap();
            let (mut cold, server) =
                spawn_session(&[&workspace_root], json!({}), options.clone()).await;
            let publish = harness::open_published(&mut cold, &uri, 1, source).await;
            assert_eq!(
                publish["params"]["diagnostics"], latest["params"]["diagnostics"],
                "{mode}, {content:?}: cold config"
            );
            harness::join_session(cold, server).await;
        }
    }
}

#[tokio::test]
async fn custom_configuration_is_watched_and_reloaded_after_path_changes() {
    for relative in [true, false] {
        let fixture = FixtureProject::empty().unwrap();
        let outside = FixtureProject::empty().unwrap();
        let initial = fixture
            .write_file("config/custom[1].toml", "ignore = [\"RY010\"]\n")
            .unwrap();
        let replacement = outside
            .write_file("replacement.toml", "ignore = []\n")
            .unwrap();
        let source = "x <- never_bound_here\n";
        let path = fixture.write_file("main.R", source).unwrap();
        let uri = file_uri(&path);
        let (mut session, server) = spawn_session(&[fixture.root()], json!({"workspace": {
            "configuration": true,
            "didChangeWatchedFiles": {"dynamicRegistration": true, "relativePatternSupport": relative}
        }}), None).await;
        // The first pull can supply a path absent from initializationOptions.
        session
            .respond_to_request(
                "workspace/configuration",
                json!([
                    {"configuration":"config/../config/custom[1].toml"}, {}
                ]),
            )
            .await
            .unwrap();
        let request = session
            .respond_to_request("client/registerCapability", json!(null))
            .await
            .unwrap();
        let patterns = &request["params"]["registrations"][0]["registerOptions"]["watchers"];
        let pattern = &patterns.as_array().unwrap().last().unwrap()["globPattern"];
        if relative {
            assert_eq!(
                pattern["baseUri"],
                initial
                    .parent()
                    .and_then(|p| tower_lsp::lsp_types::Url::from_directory_path(p).ok())
                    .unwrap()
                    .to_string()
            );
            assert_eq!(pattern["pattern"], "custom[[]1[]].toml");
        } else {
            assert_eq!(
                pattern,
                &json!(
                    initial
                        .to_string_lossy()
                        .replace('[', "[[]")
                        .replace("1]", "1[]]")
                )
            );
        }
        let initial_diagnostics = harness::open_published(&mut session, &uri, 1, source).await;
        assert_eq!(count_code(&initial_diagnostics, "RY010"), 0);

        for (contents, expected) in [
            ("ignore = []\n", 1),
            ("not valid TOML [", 1),
            ("ignore = [\"RY010\"]\n", 0),
        ] {
            std::fs::write(&initial, contents).unwrap();
            let mark = session.publication_mark();
            notify_changed(&mut session, &file_uri(&initial)).await;
            let after = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            assert_eq!(count_code(&after, "RY010"), expected);
        }
        let mark = session.publication_mark();
        session
            .notify("workspace/didChangeConfiguration", json!({"settings":{}}))
            .await
            .unwrap();
        session
            .respond_to_request(
                "workspace/configuration",
                json!([
                    {"configuration":replacement}, {}
                ]),
            )
            .await
            .unwrap();
        session
            .respond_to_request("client/unregisterCapability", json!(null))
            .await
            .unwrap();
        let registration = session
            .respond_to_request("client/registerCapability", json!(null))
            .await
            .unwrap();
        let watchers = &registration["params"]["registrations"][0]["registerOptions"]["watchers"];
        let pattern = &watchers.as_array().unwrap().last().unwrap()["globPattern"];
        if relative {
            assert_eq!(
                pattern["baseUri"],
                tower_lsp::lsp_types::Url::from_directory_path(outside.root())
                    .unwrap()
                    .to_string()
            );
            assert_eq!(pattern["pattern"], "replacement.toml");
        } else {
            assert_eq!(pattern, &json!(replacement));
        }
        let after = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert_eq!(count_code(&after, "RY010"), 1);
        std::fs::write(&replacement, "ignore = [\"RY010\"]\n").unwrap();
        let mark = session.publication_mark();
        notify_changed(&mut session, &file_uri(&replacement)).await;
        let after = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert_eq!(count_code(&after, "RY010"), 0);
        harness::join_session(session, server).await;
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Typeshed stub reload
//
// Removing the last custom typeshed directory empties the stub map, like a
// fresh server on the same config. Only a reload where every configured
// directory failed keeps the old stubs.
// ════════════════════════════════════════════════════════════════════════════

/// The stub types `my_func()` as returning a length-one list, so
/// `if (my_func())` fires RY001 (non-logical condition) exactly while
/// the stubs are applied — the same probe as
/// `invalid_root_rytoml_degrades_entirely_to_defaults`.
#[tokio::test]
async fn stub_reload_converges_when_last_typeshed_directory_is_removed() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "typeshed = [\"stubs\"]\n")
        .unwrap();
    let stub = serde_json::to_string(&json!({
        "schema_version": "1",
        "package": "localdep",
        "version": "test",
        "functions": {
            "my_func": {"params": [], "return": {"mode": "list", "length": "1"}}
        }
    }))
    .unwrap();
    fixture.write_file("stubs/localdep.json", &stub).unwrap();
    let source = "library(localdep)\nif (my_func()) print(1)\n";
    fixture.write_file("main.R", source).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let uri = file_uri(&fixture.path("main.R"));
    let config_uri = file_uri(&fixture.path("ry.toml"));

    let initial = harness::open_published(&mut session, &uri, 1, source).await;
    assert_eq!(count_code(&initial, "RY001"), 1, "{initial}");

    // Drive the watched-file reload channel through the interesting
    // transitions. Expected counts are RY001 occurrences: the stub
    // applies (1) or not (0).
    for (config, expected, why) in [
        // A configured-but-unreadable directory is a genuine reload
        // failure: the last valid stubs are retained, deliberately
        // diverging from a cold server (which starts with none).
        ("typeshed = [\"missing\"]\n", 1, "failed reload retains"),
        // Clearing the list and deleting the setting both declare no
        // directories: the empty reload is the new state.
        ("typeshed = []\n", 0, "clearing the list"),
        ("", 0, "deleting the setting"),
        // Restoring the directory reloads the stubs.
        ("typeshed = [\"stubs\"]\n", 1, "restoring"),
        // Back to empty so the cold comparison below checks the
        // sticky transition itself.
        ("typeshed = []\n", 0, "clearing again"),
    ] {
        fixture.write_file("ry.toml", config).unwrap();
        harness::sync_barrier(&mut session, &uri).await;
        let mark = session.publication_mark();
        notify_changed(&mut session, &config_uri).await;
        let after = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&after, "RY001"),
            expected,
            "{why} ({config:?}): {after}"
        );
    }
    harness::join_session(session, server).await;

    // Warm/cold convergence: a fresh server on the final config
    // (no typeshed directories) must agree with the warm session.
    let (mut cold, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let publish = harness::open_published(&mut cold, &uri, 1, source).await;
    assert_eq!(
        count_code(&publish, "RY001"),
        0,
        "cold server on the emptied config: {publish}"
    );
    harness::join_session(cold, server).await;
}

/// A configuration pull queued behind a folder addition covers the added
/// folder: the scopes are read after the preceding folder change applied.
#[tokio::test]
async fn configuration_pull_covers_a_folder_added_before_it() {
    let fixture = FixtureProject::empty().unwrap();
    let added = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", "w <- 1L\n").unwrap();
    let (mut session, server) = spawn_session(
        &[fixture.root()],
        json!({ "workspace": { "configuration": true } }),
        None,
    )
    .await;
    session
        .respond_to_request("workspace/configuration", json!([{}, {}]))
        .await
        .unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    harness::open_published(&mut session, &main_uri, 1, "w <- 1L\n").await;

    // Park a full scan, then queue a folder addition and a configuration
    // change behind it.
    ry_lsp::test_seam::arm(Seam::Scan);
    let fake = format!("{}/DESCRIPTION", file_uri(fixture.root()));
    notify_changed(&mut session, &fake).await;
    ry_lsp::test_seam::wait_arrived(Seam::Scan).await;
    let added_uri = file_uri(added.root());
    session
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({"event": {"added": [{"uri": added_uri, "name": "added"}], "removed": []}}),
        )
        .await
        .unwrap();
    session
        .notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": {} }),
        )
        .await
        .unwrap();
    ry_lsp::test_seam::release(Seam::Scan);
    let request = session
        .respond_to_request("workspace/configuration", json!([{}, {}, {}]))
        .await
        .unwrap();
    let scopes: Vec<&str> = request["params"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["scopeUri"].as_str())
        .collect();
    assert!(
        scopes
            .iter()
            .any(|scope| scope.trim_end_matches('/') == added_uri.trim_end_matches('/')),
        "the pull must include the folder added before it: {scopes:?}"
    );
    harness::join_session(session, server).await;
}
