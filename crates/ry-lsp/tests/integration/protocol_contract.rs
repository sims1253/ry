//! Multi-root, reload, and cache contracts. Each test compares the editor's
//! publications with `ry check` or with a fresh server on the same files.

use ry_lsp::test_seam::Seam;
use ry_testkit::{AsyncJsonRpcClient, FixtureProject, normalize_path};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use crate::harness;

use harness::{
    Published, cli_diagnostics, file_uri, join_session, open_published, published_from_lsp,
    spawn_session, sync_barrier,
};

#[tokio::test]
async fn assignment_hints_keep_source_types_across_edits_and_ranges() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", "").unwrap();
    let uri = file_uri(&fixture.path("main.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    session
        .open(&uri, 1, "x <- 1L\nx <- \"later\"\n")
        .await
        .unwrap();
    let request = json!({
        "textDocument": {"uri": uri},
        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 10}}
    });
    for _ in 0..2 {
        let hints = session
            .request("textDocument/inlayHint", request.clone())
            .await
            .unwrap();
        assert_eq!(hints.as_array().unwrap().len(), 1, "{hints}");
        assert_eq!(hints[0]["label"], ": integer<len=1>");
    }
    session
        .change(&uri, 2, json!([{"text": "x <- FALSE\nx <- 1L\n"}]))
        .await
        .unwrap();
    let hints = session
        .request("textDocument/inlayHint", request)
        .await
        .unwrap();
    assert_eq!(hints.as_array().unwrap().len(), 1, "{hints}");
    assert_eq!(hints[0]["label"], ": logical<len=1>");
    join_session(session, server).await;
}

#[tokio::test]
async fn assignment_hints_refresh_when_local_stubs_change() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "typeshed = [\"stubs\"]\n")
        .unwrap();
    let write_stub =
        |mode| {
            fixture.write_file("stubs/localdep.json", serde_json::to_string(&json!({
            "schema_version": "1", "package": "localdep", "version": "test",
            "functions": {"value": {"params": [], "return": {"mode": mode, "length": "1"}}}
        })).unwrap()).unwrap();
        };
    write_stub("integer");
    fixture.write_file("main.R", "").unwrap();
    let uri = file_uri(&fixture.path("main.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    session
        .open(&uri, 1, "x <- localdep::value()\n")
        .await
        .unwrap();
    let request = json!({
        "textDocument": {"uri": uri},
        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 0}}
    });
    let hints = session
        .request("textDocument/inlayHint", request.clone())
        .await
        .unwrap();
    assert_eq!(hints[0]["label"], ": integer<len=1>");
    // Finish the initial publication before using the config-triggered
    // publication as proof that the asynchronous reload has completed.
    session.published_diagnostics_after(&uri, 0).await.unwrap();
    write_stub("character");
    let mark = session.publication_mark();
    session
        .notify("workspace/didChangeConfiguration", json!({"settings": {}}))
        .await
        .unwrap();
    session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    let hints = session
        .request("textDocument/inlayHint", request)
        .await
        .unwrap();
    assert_eq!(hints[0]["label"], ": character<len=1>");
    join_session(session, server).await;
}

fn cli_diagnostics_in_dir(dir: &Path, extra: &[&str]) -> Vec<Published> {
    cli_diagnostics(dir, ".", extra)
}

/// Open `R/main.R` in each root and return its publication, relative to
/// that root.
async fn open_each_main(
    session: &mut harness::ClientSession,
    roots: &[PathBuf],
) -> Vec<Vec<Published>> {
    let mut published = Vec::new();
    for root in roots {
        let path = root.join("R/main.R");
        let text = std::fs::read_to_string(&path).unwrap();
        let publish = open_published(session, &file_uri(&path), 1, &text).await;
        published.push(published_from_lsp(&publish, &path, root));
    }
    published
}

// ──────────────────────────────────────────────────────────────────────────
// Per-folder settings and stubs
// ──────────────────────────────────────────────────────────────────────────

/// Each root publishes what `ry check` reports in that root. Root A ignores
/// RY002 in `ry.toml` and in its editor settings. Root B does not.
#[tokio::test]
async fn two_roots_different_editor_settings_differential() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("root-a/ry.toml", "ignore = [\"RY002\"]\n")
        .unwrap();
    for root in ["root-a", "root-b"] {
        fixture
            .write_file(format!("{root}/R/main.R"), "if (c(TRUE, FALSE)) print(1)\n")
            .unwrap();
    }
    let roots = [fixture.path("root-a"), fixture.path("root-b")];
    let cli: Vec<_> = roots
        .iter()
        .map(|root| cli_diagnostics(root, ".", &[]))
        .collect();
    assert_ne!(cli[0], cli[1], "roots must produce different CLI output");

    let (mut session, server) = spawn_session(
        &[&roots[0], &roots[1]],
        json!({}),
        Some(json!({
            "settings": [{"lint": {"ignore": ["RY002"]}}, {}],
            "globalSettings": {}
        })),
    )
    .await;
    let lsp = open_each_main(&mut session, &roots).await;
    join_session(session, server).await;
    assert_eq!(lsp, cli, "each root must match an independent CLI run");
}

/// Two roots define the stub package `localdep` with different return types
/// for `my_func`. Root B's list return makes `if (my_func())` fire RY001.
/// Each root publishes what `ry check` reports in that root.
#[tokio::test]
async fn colliding_local_stubs_isolation() {
    let fixture = FixtureProject::empty().unwrap();
    for (root, return_mode) in [("root-a", "integer"), ("root-b", "list")] {
        fixture
            .write_file(
                format!("{root}/DESCRIPTION"),
                "Package: testpkg\nVersion: 0.0.1\nImports: localdep\n",
            )
            .unwrap();
        fixture
            .write_file(
                format!("{root}/NAMESPACE"),
                "importFrom(localdep, my_func)\nexport(use_it)\n",
            )
            .unwrap();
        fixture
            .write_file(format!("{root}/ry.toml"), "typeshed = [\"stubs\"]\n")
            .unwrap();
        let stub = serde_json::to_string(&json!({
            "schema_version": "1",
            "package": "localdep",
            "version": "test",
            "functions": {
                "my_func": {
                    "params": [],
                    "return": {"mode": return_mode, "length": "1"}
                }
            }
        }))
        .unwrap();
        fixture
            .write_file(format!("{root}/stubs/localdep.json"), &stub)
            .unwrap();
        fixture
            .write_file(format!("{root}/R/main.R"), "if (my_func()) print(1)\n")
            .unwrap();
    }
    let roots = [fixture.path("root-a"), fixture.path("root-b")];
    let cli: Vec<_> = roots
        .iter()
        .map(|root| cli_diagnostics(root, ".", &[]))
        .collect();
    assert_ne!(cli[0], cli[1], "roots must produce different CLI output");

    let (mut session, server) = spawn_session(&[&roots[0], &roots[1]], json!({}), None).await;
    let lsp = open_each_main(&mut session, &roots).await;
    join_session(session, server).await;
    assert_eq!(lsp, cli, "each root must match an independent CLI run");
}

/// Removing a workspace folder clears the diagnostics of its open document:
/// the next publication for it is empty, or none arrives.
#[tokio::test]
async fn workspace_folder_add_remove_convergence() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("root-a/R/main.R", "length(xx = 1L)\n")
        .unwrap();
    fixture
        .write_file("root-b/R/main.R", "length(xx = 1L)\n")
        .unwrap();
    let root_a = fixture.path("root-a");
    let root_b = fixture.path("root-b");
    let main_a_uri = file_uri(&root_a.join("R/main.R"));
    let main_b_uri = file_uri(&root_b.join("R/main.R"));
    let (mut session, server) = spawn_session(&[&root_a, &root_b], json!({}), None).await;
    sync_barrier(&mut session, &main_a_uri).await;
    open_published(&mut session, &main_b_uri, 1, "length(xx = 1L)\n").await;

    session
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({
                "event": {
                    "added": [],
                    "removed": [{"uri": file_uri(&root_b), "name": "root-b"}]
                }
            }),
        )
        .await
        .unwrap();
    sync_barrier(&mut session, &main_a_uri).await;
    let clear_mark = session.publication_mark();
    let republish = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        session.published_diagnostics_after(&main_b_uri, clear_mark),
    )
    .await;
    join_session(session, server).await;

    match republish {
        Ok(Ok(publish)) => {
            let diags = publish
                .pointer("/params/diagnostics")
                .and_then(Value::as_array);
            assert!(
                diags.is_none_or(|d| d.is_empty()),
                "root-b diagnostics must be cleared after removal; got: {:?}",
                publish.pointer("/params/diagnostics")
            );
        }
        Ok(Err(e)) => panic!("transport error during republish: {e}"),
        Err(_) => { /* timeout: server quiesced, diagnostics may already be cleared */ }
    }
}

/// A stale parse result must not replace a newer tree cache
/// entry. The forced sequence is:
///   1. parse version N starts;
///   2. `didChange` installs N+1;
///   3. parse N finishes;
///   4. stale result is rejected;
///   5. diagnostics equal a fresh parse of N+1.
///
/// The test-only scheduler/barrier seam (`ry_lsp::test_seam`) forces this
/// ordering without any sleeps. The seam controls scheduling only. The
/// rejection is production code: `backend::parsed_file` keeps a parse only
/// if the document's edit stamp has not changed.
#[tokio::test]
async fn version_stamped_tree_cache_rejects_stale_parse() {
    let fixture = FixtureProject::empty().unwrap();
    let source_v1 = "x <- 1L\ny <- 2L\n";
    let source_v2 = "z <- length(xx = 1L)\n"; // triggers RY090 after edit
    fixture.write_file("main.R", source_v1).unwrap();

    let main_uri = file_uri(&fixture.path("main.R"));
    let (mut live, server) = spawn_session(&[fixture.root()], json!({}), None).await;

    // Force the parse/didChange interleaving; the sequence is documented
    // at the `maybe_pause` call site in `backend::parsed_file`.
    ry_lsp::test_seam::arm(Seam::Parse);
    ry_lsp::test_seam::arm(Seam::DidChange);

    // Open version 1. `schedule_diagnostics` debounces 180 ms, then
    // `publish_diagnostics` calls `parsed_file` → barrier pauses.
    live.open(&main_uri, 1, source_v1).await.unwrap();

    // Wait for the parse of version 1 to start: `parsed_file` has read
    // the v1 text/version/tree and is now paused.
    ry_lsp::test_seam::wait_arrived(Seam::Parse).await;

    // Install version 2 while the version-1 parse is paused.
    live.change(&main_uri, 2, json!([{"text": source_v2}]))
        .await
        .unwrap();

    // Wait for didChange to be fully processed: document updated,
    // version bumped, diagnostics re-scheduled. This sync point is
    // necessary because tower-lsp dispatches handlers concurrently —
    // without it the barrier release could race ahead of the document
    // update and the stale parse would not be detected.
    ry_lsp::test_seam::wait_landed(Seam::DidChange).await;

    // Release the barrier: the version-1 parse finishes, `parsed_file`
    // discards it because the document's edit stamp moved, and the retry
    // loop parses version 2 fresh.
    ry_lsp::test_seam::release(Seam::Parse);

    // Collect diagnostics for version 2. The didChange triggered
    // `schedule_diagnostics(gen=2)`, which publishes after the debounce.
    let mark = live.publication_mark();
    let publish_v2 = live
        .published_diagnostics_after(&main_uri, mark)
        .await
        .unwrap();

    let v2_codes: Vec<&str> = publish_v2["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert!(
        v2_codes.contains(&"RY090"),
        "version 2 must produce RY090 after the stale parse is rejected; got: {v2_codes:?}"
    );
    join_session(live, server).await;
}

// ──────────────────────────────────────────────────────────────────────────
// Baseline reload
//
// The baseline is loaded into each folder context and reloaded on watch
// events. A failed reload keeps the last valid context.
// ──────────────────────────────────────────────────────────────────────────

fn diagnostic_codes(publish: &Value) -> Vec<String> {
    publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap_or("").to_string())
        .collect()
}

/// A corrupt baseline keeps the last valid one, so RY002 stays suppressed.
/// A valid empty baseline then lets RY002 back, matching a fresh server.
#[tokio::test]
async fn baseline_reload_keeps_the_last_valid_baseline_and_converges() {
    const SOURCE: &str = "if (c(TRUE, FALSE)) print(1)\n";
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "baseline = \"baseline.json\"\n")
        .unwrap();
    fixture.write_file(
        "baseline.json",
        r#"{"version": 1, "entries": [{"path": "main.R", "code": "RY002", "message": "`if` condition has length 2; R requires a length-1 condition", "count": 1}]}"#,
    ).unwrap();
    fixture.write_file("main.R", SOURCE).unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    let baseline_uri = file_uri(&fixture.path("baseline.json"));
    let (mut live, server) = spawn_session(&[fixture.root()], json!({}), None).await;

    let opened = open_published(&mut live, &main_uri, 1, SOURCE).await;
    assert!(
        !diagnostic_codes(&opened).contains(&"RY002".to_string()),
        "the valid baseline must suppress RY002: {opened}"
    );

    let mut reloaded = Vec::new();
    for (version, baseline) in [
        (2, "CORRUPT NOT JSON"),
        (3, r#"{"version": 1, "entries": []}"#),
    ] {
        std::fs::write(fixture.path("baseline.json"), baseline).unwrap();
        harness::notify_changed(&mut live, &baseline_uri).await;
        sync_barrier(&mut live, &main_uri).await;
        reloaded = diagnostic_codes(
            &harness::change_published(&mut live, &main_uri, version, SOURCE).await,
        );
        assert_eq!(
            reloaded.contains(&"RY002".to_string()),
            version == 3,
            "after writing {baseline:?}: {reloaded:?}"
        );
    }
    join_session(live, server).await;

    let (mut fresh, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let fresh_codes = diagnostic_codes(&open_published(&mut fresh, &main_uri, 1, SOURCE).await);
    join_session(fresh, server).await;
    assert_eq!(
        reloaded, fresh_codes,
        "the reload must converge to a fresh server"
    );
}

/// Publication and inlay hints use the baseline loaded with the folder
/// context at initialize. Emptying the file afterwards, with no
/// didChangeWatchedFiles to trigger a reload, must not bring back the
/// finding it suppresses.
#[tokio::test]
async fn publish_uses_the_baseline_loaded_at_initialize() {
    use ry_testkit::LspSession;

    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("ry.toml", "baseline = \"baseline.json\"\n")
        .unwrap();
    fixture.write_file(
        "baseline.json",
        r#"{"version": 1, "entries": [{"path": "main.R", "code": "RY002", "message": "`if` condition has length 2; R requires a length-1 condition", "count": 1}]}"#,
    ).unwrap();
    fixture
        .write_file("main.R", "if (c(TRUE, FALSE)) print(1)\n")
        .unwrap();

    let main_uri = file_uri(&fixture.path("main.R"));

    let (cs, ss) = tokio::io::duplex(128 * 1024);
    let (cr, cw) = tokio::io::split(cs);
    let (sr, sw) = tokio::io::split(ss);
    let server = tokio::spawn(async move { ry_lsp::run_with(sr, sw).await });
    let mut live = LspSession::new(cr, cw);
    live.initialize(fixture.root()).await.unwrap();

    fixture
        .write_file("baseline.json", r#"{"version": 1, "entries": []}"#)
        .unwrap();

    // Exercise inlay hints (read path) and publish (diagnostic path).
    live.request(
        "textDocument/inlayHint",
        json!({
            "textDocument": {"uri": main_uri},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
        }),
    )
    .await
    .ok();
    let mark = live.publication_mark();
    live.open(&main_uri, 1, "if (c(TRUE, FALSE)) print(1)\n")
        .await
        .unwrap();
    let opened = live
        .published_diagnostics_after(&main_uri, mark)
        .await
        .unwrap();
    let mark2 = live.publication_mark();
    live.change(
        &main_uri,
        2,
        json!([{"text": "if (c(TRUE, FALSE)) print(1)\n"}]),
    )
    .await
    .unwrap();
    let changed = live
        .published_diagnostics_after(&main_uri, mark2)
        .await
        .unwrap();
    let _ = live.shutdown().await;
    drop(live);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;

    for publication in [opened, changed] {
        assert_eq!(
            harness::count_code(&publication, "RY002"),
            0,
            "{publication}"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Invalid root ry.toml
// ──────────────────────────────────────────────────────────────────────────

/// A root `ry.toml` that parses but fails `Config::validate` (here
/// `[index] max-files = 0`) degrades to defaults with no user stubs:
/// initialize succeeds, diagnostics publish, and none of its `ignore`,
/// `error`, or `typeshed` entries apply. The probe document lives outside
/// every workspace folder, so it is checked with the root-level config.
///
/// A control session on a valid root with the same stubs shows the probe
/// fires RY001 when the stubs apply, so the RY001 absence is not vacuous.
#[tokio::test]
async fn invalid_root_rytoml_degrades_entirely_to_defaults() {
    let fixture = FixtureProject::empty().unwrap();
    // Top-level keys precede `[index]` so the file deserializes fully as
    // a `Config` and `validate()` rejects it, not the TOML parser.
    fixture
        .write_file(
            "broken/ry.toml",
            "ignore = [\"RY002\"]\nerror = [\"RY002\"]\ntypeshed = [\"stubs\"]\n\n[index]\nmax-files = 0\n",
        )
        .unwrap();
    fixture
        .write_file("valid/ry.toml", "typeshed = [\"stubs\"]\n")
        .unwrap();
    let stub = serde_json::to_string(&json!({
        "schema_version": "1",
        "package": "localdep",
        "version": "test",
        "functions": {
            "my_func": {
                "params": [],
                "return": {"mode": "list", "length": "1"}
            }
        }
    }))
    .unwrap();
    fixture
        .write_file("broken/stubs/localdep.json", &stub)
        .unwrap();
    fixture
        .write_file("valid/stubs/localdep.json", &stub)
        .unwrap();

    // Line 1 probes the filter and severity. Lines 2-3 probe the stubs:
    // a list return from `my_func` fires RY001 on line 3.
    let probe = "if (c(TRUE, FALSE)) print(1)\nlibrary(localdep)\nif (my_func()) print(1)\n";
    fixture.write_file("outside/main.R", probe).unwrap();
    let doc_uri = file_uri(&fixture.path("outside/main.R"));

    /// The (code, severity) pairs published for the probe in a session on
    /// `root`.
    async fn open_unowned_and_collect(root: &Path, uri: &str, text: &str) -> Vec<(String, u64)> {
        let (mut session, server) = harness::spawn_server();
        let init = session
            .initialize(root)
            .await
            .expect("initialize must succeed despite an invalid root ry.toml");
        assert!(
            init.get("capabilities").is_some(),
            "initialize must return capabilities despite an invalid root ry.toml"
        );
        let publish = open_published(&mut session, uri, 1, text).await;
        join_session(session, server).await;
        publish["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                (
                    d["code"].as_str().unwrap_or("").to_string(),
                    d["severity"].as_u64().unwrap_or(0),
                )
            })
            .collect()
    }

    let valid_pairs = open_unowned_and_collect(&fixture.path("valid"), &doc_uri, probe).await;
    assert!(
        valid_pairs.iter().any(|(code, _)| code == "RY001"),
        "control: valid root config must apply the stubs and fire RY001; got {valid_pairs:?}"
    );
    assert!(
        valid_pairs.iter().any(|(code, _)| code == "RY002"),
        "control: RY002 must fire under the valid root config; got {valid_pairs:?}"
    );

    let broken_pairs = open_unowned_and_collect(&fixture.path("broken"), &doc_uri, probe).await;
    assert!(
        broken_pairs.iter().any(|(code, _)| code == "RY002"),
        "default filter must hold: RY002 must survive the broken config's ignore entry; got {broken_pairs:?}"
    );
    // RY002 stays at its default Warning (2), not Error (1).
    assert!(
        broken_pairs
            .iter()
            .filter(|(code, _)| code == "RY002")
            .all(|(_, severity)| *severity == 2),
        "default severity must hold: RY002 must publish as Warning(2), not Error(1); got {broken_pairs:?}"
    );
    assert!(
        !broken_pairs.iter().any(|(code, _)| code == "RY001"),
        "no typeshed stubs from the broken config: RY001 must be absent; got {broken_pairs:?}"
    );
}

/// Several files with different diagnostics in one folder each publish
/// exactly what `ry check` reports for that file. Differing codes, lines,
/// and counts make a mix-up between files visible.
#[tokio::test]
async fn per_file_publication_matches_cli_across_differing_files() {
    let fixture = FixtureProject::empty().unwrap();
    let files: &[(&str, &str)] = &[
        // RY090 (partial argument name) at line 0.
        ("a.R", "z <- length(xx = 1L)\n"),
        // RY002 (length-2 if condition) at line 0.
        ("b.R", "if (c(TRUE, FALSE)) print(1)\n"),
        // Both triggers, at lines 1 and 2.
        (
            "c.R",
            "w <- 1L\nif (c(TRUE, FALSE)) print(1)\nz <- length(xx = 1L)\n",
        ),
        // Clean: publishes an empty set.
        ("d.R", "ok <- 1L\n"),
    ];
    for (name, text) in files {
        fixture.write_file(*name, *text).unwrap();
    }

    let cli = cli_diagnostics(fixture.root(), ".", &[]);
    let code_lines = |path: &str| -> std::collections::BTreeSet<String> {
        cli.iter()
            .filter(|d| d.path == path)
            .map(|d| format!("{}@{}", d.code, d.line))
            .collect()
    };
    let (a, b, c) = (code_lines("a.R"), code_lines("b.R"), code_lines("c.R"));
    assert!(
        !a.is_empty() && !b.is_empty() && !c.is_empty(),
        "fixture must produce diagnostics; CLI said: {cli:?}"
    );
    assert!(
        a != b && a != c && b != c,
        "files must produce differing diagnostics; got {a:?}, {b:?}, {c:?}"
    );

    let (mut client, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    // Opening the clean file publishes every indexed file in one pass.
    let mark = client.publication_mark();
    client
        .open(&file_uri(&fixture.path("d.R")), 1, "ok <- 1L\n")
        .await
        .unwrap();
    let mut lsp: Vec<Published> = Vec::new();
    for (name, _) in files {
        let uri = file_uri(&fixture.path(name));
        let publish = client
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        lsp.extend(published_from_lsp(
            &publish,
            &fixture.path(name),
            fixture.root(),
        ));
    }
    lsp.sort();
    join_session(client, server).await;

    assert_eq!(
        lsp, cli,
        "every file's publication must equal the CLI's result for that file"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Shared discovery
// ──────────────────────────────────────────────────────────────────────────

/// (#48): the same fixture tree must produce the same discovered path
/// set in CLI and LSP. The fixture includes a `target/` directory, a hidden
/// directory (both skip), an opened file, and a closed file. Every file has
/// a diagnostic so the discovery set is observable through published
/// diagnostic paths. The closed `other.R` must be published, so the
/// absence of the skipped files means the LSP did not discover them.
#[tokio::test]
async fn cli_lsp_discovery_set_equality() {
    let fixture = FixtureProject::empty().unwrap();
    // Normal files with a diagnostic. Only `normal.R` is opened.
    fixture.write_file("normal.R", "length(xx = 1L)\n").unwrap();
    fixture.write_file("other.R", "length(xx = 1L)\n").unwrap();
    // File in target/ — both modes skip this directory.
    fixture
        .write_file("target/skipped.R", "length(xx = 1L)\n")
        .unwrap();
    // Hidden directory — both skip.
    fixture
        .write_file(".hidden/secret.R", "length(xx = 1L)\n")
        .unwrap();

    let root = fixture.root();

    // CLI discovery set: paths that received diagnostics.
    let cli_diags = cli_diagnostics_in_dir(root, &[]);
    let cli_paths: std::collections::BTreeSet<String> =
        cli_diags.iter().map(|d| d.path.clone()).collect();

    let root_uri = file_uri(root);
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
    let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

    let init_id = client
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
            }),
        )
        .await
        .unwrap();
    client
        .receive_until(|m| m.get("id") == Some(&json!(init_id)), 16)
        .await
        .unwrap();
    client.notify("initialized", json!({})).await.unwrap();

    // Open normal.R to trigger the background index.
    let normal_uri = file_uri(&fixture.path("normal.R"));
    let normal_text = std::fs::read_to_string(fixture.path("normal.R")).unwrap();
    client.notify("textDocument/didOpen", json!({
        "textDocument": {"uri": normal_uri, "languageId": "r", "version": 1, "text": normal_text}
    })).await.unwrap();

    // Record every published path, in any order.
    let mut lsp_uris: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let record = |lsp_uris: &mut std::collections::BTreeSet<String>, message: &Value| {
        if message.get("method") != Some(&json!("textDocument/publishDiagnostics")) {
            return;
        }
        if let Some(uri) = message.pointer("/params/uri").and_then(Value::as_str) {
            let path = uri.strip_prefix("file://").unwrap_or(uri);
            lsp_uris.insert(normalize_path(Path::new(path), root));
        }
    };
    // Wait for every path the CLI found, with a deadline. An idle gap between
    // publications does not prove the background pass is over.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !cli_paths.is_subset(&lsp_uris) {
            let message = client.receive().await.unwrap();
            record(&mut lsp_uris, &message);
        }
    })
    .await;

    // Then drain any extra publications the CLI does not expect (bounded).
    for _ in 0..128 {
        match tokio::time::timeout(std::time::Duration::from_millis(500), client.receive()).await {
            Ok(Ok(message)) => {
                record(&mut lsp_uris, &message);
            }
            Ok(Err(e)) => panic!("transport error during drain: {e}"),
            Err(_) => break, // timeout: server has quiesced
        }
    }

    let shutdown_id = client.request("shutdown", Value::Null).await.unwrap();
    client
        .receive_until(|m| m.get("id") == Some(&json!(shutdown_id)), 16)
        .await
        .unwrap();
    client.notify("exit", Value::Null).await.unwrap();
    drop(client);
    let code = tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(code, std::process::ExitCode::SUCCESS);

    // Convert LSP URIs to normalized paths and compare with CLI.
    assert_eq!(
        cli_paths, lsp_uris,
        "CLI and LSP must discover the same file set; \
         CLI: {cli_paths:?}, LSP: {lsp_uris:?}"
    );
}
