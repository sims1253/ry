//! Adopted source annotations use the same warm checking path in the editor.

mod harness;

use harness::{join_session, spawn_session, sync_barrier};
use ry_testkit::{FixtureProject, file_uri};
use serde_json::{Value, json};

const ADOPT: &str = "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n";

/// Two native filenames that share the display path `R/bad\u{fffd}.R`.
#[cfg(unix)]
fn colliding_paths(fixture: &FixtureProject) -> [std::path::PathBuf; 2] {
    use std::os::unix::ffi::OsStringExt;
    [b"bad\xff.R".as_slice(), b"bad\xfe.R".as_slice()].map(|name| {
        fixture
            .path("R")
            .join(std::ffi::OsString::from_vec(name.to_vec()))
    })
}

fn count_code(publish: &Value, code: &str) -> usize {
    publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["code"] == code)
        .count()
}

#[tokio::test]
async fn annotation_only_editor_edits_retract_and_restore_mismatch() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ADOPT).unwrap();
    let source = |annotation: &str| {
        format!(
            "label <- \"\u{1f600}\"\nf <- function(x) {{\n  {annotation}\n  x\n}}\nf(\"bad\")\n"
        )
    };
    let first = source("#| x integer");
    let path = fixture.write_file("R/main.R", &first).unwrap();
    let uri = file_uri(&path).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;

    let mark = session.publication_mark();
    session.open(&uri, 1, &first).await.unwrap();
    let initial = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&initial, "RY114"), 1, "{initial}");
    let mismatch = initial["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "RY114")
        .unwrap();
    assert_eq!(
        mismatch["range"]["start"],
        json!({"line": 5, "character": 2})
    );

    for (version, annotation, expected) in [
        (2, "#| x character", 0),
        (3, "# ordinary comment", 0),
        (4, "#| x integer", 1),
        (5, "NULL #| x integer", 0),
        (6, "#| x integer", 1),
    ] {
        let mark = session.publication_mark();
        session
            .change(&uri, version, json!([{"text": source(annotation)}]))
            .await
            .unwrap();
        let publish = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert_eq!(count_code(&publish, "RY114"), expected, "{publish}");
    }
    let config_uri = file_uri(&fixture.path("ry.toml")).unwrap();
    for (config, expected) in [
        (
            "[annotations.typehint]\nadopt = false\nversion = '0.1.0'\npaths = ['R/**']\n",
            0,
        ),
        (
            "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['other/**']\n",
            0,
        ),
        (
            "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
            1,
        ),
    ] {
        fixture.write_file("ry.toml", config).unwrap();
        sync_barrier(&mut session, &uri).await;
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": config_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        let publish = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert_eq!(count_code(&publish, "RY114"), expected, "{publish}");
    }
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn native_filename_and_unicode_collision_never_adopt_the_wrong_contract() {
    use std::os::unix::ffi::OsStringExt;

    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ADOPT).unwrap();
    std::fs::create_dir(fixture.path("R")).unwrap();
    let source = "f <- function(x) {\n #| x integer\n x\n}\nf(\"bad\")\n";
    let raw = fixture
        .path("R")
        .join(std::ffi::OsString::from_vec(b"bad\xff.R".to_vec()));
    std::fs::write(&raw, source).unwrap();
    let raw_uri = file_uri(&raw).unwrap();

    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&raw_uri, 1, source).await.unwrap();
    let native_only = session
        .published_diagnostics_after(&raw_uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&native_only, "RY114"), 0, "{native_only}");
    assert_eq!(count_code(&native_only, "RY117"), 1, "{native_only}");
    assert_eq!(native_only["params"]["uri"], raw_uri);
    let without_annotation = source.replace("#| x integer", "# ordinary");
    let mark = session.publication_mark();
    session
        .change(&raw_uri, 2, json!([{"text": without_annotation}]))
        .await
        .unwrap();
    let cleared = session
        .published_diagnostics_after(&raw_uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&cleared, "RY117"), 0, "{cleared}");
    let mark = session.publication_mark();
    session
        .change(&raw_uri, 3, json!([{"text": source}]))
        .await
        .unwrap();
    let restored_warning = session
        .published_diagnostics_after(&raw_uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&restored_warning, "RY117"), 1);
    join_session(session, server).await;

    let unicode = fixture.path("R/bad\u{fffd}.R");
    std::fs::write(&unicode, source).unwrap();
    let unicode_uri = file_uri(&unicode).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&raw_uri, 1, source).await.unwrap();
    let collision_native = session
        .published_diagnostics_after(&raw_uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&collision_native, "RY114"), 0);
    assert_eq!(count_code(&collision_native, "RY117"), 1);
    assert_eq!(collision_native["params"]["uri"], raw_uri);
    join_session(session, server).await;

    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&unicode_uri, 1, source).await.unwrap();
    let collision_unicode = session
        .published_diagnostics_after(&unicode_uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&collision_unicode, "RY114"), 0);
    assert_eq!(count_code(&collision_unicode, "RY117"), 1);

    fixture
        .write_file(
            "ry.toml",
            "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n\n[[rule-overrides]]\npaths = ['R/bad�.R']\nignore = ['RY117']\n",
        )
        .unwrap();
    session
        .notify("workspace/didChangeConfiguration", json!({"settings": {}}))
        .await
        .unwrap();
    sync_barrier(&mut session, &unicode_uri).await;
    let mark = session.publication_mark();
    session.open(&raw_uri, 1, source).await.unwrap();
    let scoped = session
        .quiesce_diagnostics(&raw_uri, mark, std::time::Duration::from_millis(200))
        .await
        .unwrap();
    assert_eq!(
        scoped[&unicode_uri]
            .iter()
            .filter(|d| d["code"] == "RY117")
            .count(),
        0
    );
    assert_eq!(
        scoped[&raw_uri]
            .iter()
            .filter(|d| d["code"] == "RY117")
            .count(),
        1
    );
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": raw_uri}}),
        )
        .await
        .unwrap();
    std::fs::remove_file(&raw).unwrap();
    let mark = session.publication_mark();
    session
        .change(&unicode_uri, 2, json!([{"text": source}]))
        .await
        .unwrap();
    let restored = session
        .published_diagnostics_after(&unicode_uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&restored, "RY114"), 1, "{restored}");
    assert_eq!(count_code(&restored, "RY117"), 0, "{restored}");
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn change_transition_to_collision_keeps_the_edit_with_its_original_uri() {
    use std::time::Duration;

    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ADOPT).unwrap();
    std::fs::create_dir(fixture.path("R")).unwrap();
    let first = "f <- function(x) {\n #| x integer\n x\n}\n";
    let second = "g <- function(x) {\n #| x integer\n x\n}\n";
    let paths = colliding_paths(&fixture);
    for (path, source) in paths.iter().zip([first, second]) {
        std::fs::write(path, source).unwrap();
    }
    let uris = paths.map(|path| file_uri(&path).unwrap());
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&uris[0], 1, first).await.unwrap();
    let initial = session
        .published_diagnostics_after(&uris[0], mark)
        .await
        .unwrap();
    assert_eq!(count_code(&initial, "RY117"), 1, "{initial}");

    // A's edit waits after the handler has received it. B opens at
    // the same display key before A's edit commits.
    ry_lsp::test_seam::arm_change_transition();
    session
        .change(
            &uris[0],
            2,
            json!([{"range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}
            }, "text": "# ry: ignore-file\n"}]),
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        ry_lsp::test_seam::wait_change_transition(),
    )
    .await
    .unwrap();
    let mark = session.publication_mark();
    session.open(&uris[1], 2, second).await.unwrap();
    let both = session
        .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
        .await
        .unwrap();
    assert_eq!(
        both[&uris[1]]
            .iter()
            .filter(|d| d["code"] == "RY117")
            .count(),
        1
    );
    ry_lsp::test_seam::release_change_transition();
    tokio::time::timeout(
        Duration::from_secs(10),
        ry_lsp::test_seam::wait_change_transition_landed(),
    )
    .await
    .unwrap();
    let mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uris[1]}}),
        )
        .await
        .unwrap();
    let survivor = session
        .published_diagnostics_after(&uris[0], mark)
        .await
        .unwrap();
    assert_eq!(count_code(&survivor, "RY117"), 0, "{survivor}");
    join_session(session, server).await;
}

#[tokio::test]
async fn change_waiting_on_a_close_cannot_recreate_the_closed_document() {
    use std::time::Duration;

    let fixture = FixtureProject::empty().unwrap();
    let path = fixture.write_file("main.R", "x <- 1L\n").unwrap();
    let uri = file_uri(&path).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, "x <- 1L\n").await.unwrap();
    let _ = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();

    ry_lsp::test_seam::arm_change_transition();
    session
        .change(&uri, 2, json!([{"text": "x <- FALSE\n"}]))
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        ry_lsp::test_seam::wait_change_transition(),
    )
    .await
    .unwrap();
    let mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
        .await
        .unwrap();
    let cleared = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert!(
        cleared["params"]["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    ry_lsp::test_seam::release_change_transition();
    tokio::time::timeout(
        Duration::from_secs(10),
        ry_lsp::test_seam::wait_change_transition_landed(),
    )
    .await
    .unwrap();
    let hints = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 20}}
            }),
        )
        .await
        .unwrap();
    assert!(hints.is_null(), "closed source reappeared: {hints}");
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn same_version_collision_rejects_a_parse_from_the_previous_uri() {
    use std::time::Duration;

    let fixture = FixtureProject::empty().unwrap();
    std::fs::create_dir(fixture.path("R")).unwrap();
    let sources = ["a <- 1L\n", "b <- TRUE\n"];
    let paths = colliding_paths(&fixture);
    for (path, source) in paths.iter().zip(sources) {
        std::fs::write(path, source).unwrap();
    }
    let uris = paths.map(|path| file_uri(&path).unwrap());
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    ry_lsp::test_seam::arm();
    session.open(&uris[0], 1, sources[0]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), ry_lsp::test_seam::wait_arrived())
        .await
        .unwrap();

    // Both native buffers carry version 1, but have different ASTs.
    // B's publication parses B while A's earlier parse is parked.
    let mark = session.publication_mark();
    session.open(&uris[1], 1, sources[1]).await.unwrap();
    let _ = session
        .published_diagnostics_after(&uris[1], mark)
        .await
        .unwrap();
    ry_lsp::test_seam::release_barrier();
    tokio::time::timeout(
        Duration::from_secs(10),
        ry_lsp::test_seam::wait_parse_landed(),
    )
    .await
    .unwrap();

    let hints = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uris[1]},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 20}}
            }),
        )
        .await
        .unwrap();
    assert_eq!(hints[0]["label"], ": logical<len=1>", "{hints}");
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn sole_genuine_unicode_replacement_filename_adopts_normally() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ADOPT).unwrap();
    let source = "f <- function(x) {\n #| x integer\n x\n}\nf(\"bad\")\n";
    let path = fixture.write_file("R/bad\u{fffd}.R", source).unwrap();
    let uri = file_uri(&path).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, source).await.unwrap();
    let publish = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&publish, "RY114"), 1, "{publish}");
    assert_eq!(count_code(&publish, "RY117"), 0, "{publish}");
    join_session(session, server).await;

    // An editor buffer may be new and have no on-disk twin yet.
    std::fs::remove_file(&path).unwrap();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, source).await.unwrap();
    let unsaved = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert_eq!(count_code(&unsaved, "RY114"), 1, "{unsaved}");
    assert_eq!(count_code(&unsaved, "RY117"), 0, "{unsaved}");
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn two_native_uris_keep_distinct_publication_and_survivor_ownership() {
    use std::time::Duration;

    for (reverse_open, close_second) in [(false, false), (false, true), (true, false), (true, true)]
    {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("ry.toml", ADOPT).unwrap();
        std::fs::create_dir(fixture.path("R")).unwrap();
        let source = "f <- function(x) {\n #| x integer\n x\n}\nf(\"bad\")\n";
        let raw_paths = colliding_paths(&fixture);
        for path in &raw_paths {
            std::fs::write(path, source).unwrap();
        }
        let mut uris = raw_paths.map(|path| file_uri(&path).unwrap());
        if reverse_open {
            uris.swap(0, 1);
        }
        let invented = tower_lsp::lsp_types::Url::from_file_path(fixture.path("R/bad\u{fffd}.R"))
            .unwrap()
            .to_string();
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        let mark = session.publication_mark();
        session.open(&uris[0], 1, source).await.unwrap();
        let first = session
            .published_diagnostics_after(&uris[0], mark)
            .await
            .unwrap();
        assert_eq!(count_code(&first, "RY117"), 1, "{first}");

        let mark = session.publication_mark();
        session.open(&uris[1], 1, source).await.unwrap();
        let both = session
            .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
            .await
            .unwrap();
        assert!(!both.contains_key(&invented), "{both:?}");
        for uri in &uris {
            assert_eq!(
                both.get(uri)
                    .unwrap_or_else(|| panic!("missing {uri}: {both:?}"))
                    .iter()
                    .filter(|diagnostic| diagnostic["code"] == "RY117")
                    .count(),
                1,
                "{both:?}"
            );
        }

        let closing = usize::from(close_second);
        let survivor = 1 - closing;
        let mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uris[closing]}}),
            )
            .await
            .unwrap();
        let after_close = session
            .quiesce_diagnostics(&uris[survivor], mark, Duration::from_millis(200))
            .await
            .unwrap();
        assert!(after_close.get(&uris[closing]).is_some_and(Vec::is_empty));
        assert_eq!(
            after_close[&uris[survivor]]
                .iter()
                .filter(|diagnostic| diagnostic["code"] == "RY117")
                .count(),
            1,
            "{after_close:?}"
        );
        assert!(!after_close.contains_key(&invented), "{after_close:?}");

        let mark = session.publication_mark();
        session
            .change(
                &uris[survivor],
                2,
                json!([{"range": {
                    "start": {"line": 1, "character": 1},
                    "end": {"line": 1, "character": 13}
                }, "text": "# ordinary"}]),
            )
            .await
            .unwrap();
        let edited = session
            .published_diagnostics_after(&uris[survivor], mark)
            .await
            .unwrap();
        assert_eq!(count_code(&edited, "RY117"), 0, "{edited}");

        let mark = session.publication_mark();
        session
            .change(
                &uris[survivor],
                3,
                json!([{"range": {
                    "start": {"line": 1, "character": 1},
                    "end": {"line": 1, "character": 11}
                }, "text": "#| x integer"}]),
            )
            .await
            .unwrap();
        let restored = session
            .published_diagnostics_after(&uris[survivor], mark)
            .await
            .unwrap();
        assert_eq!(count_code(&restored, "RY117"), 1, "{restored}");

        let mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uris[survivor]}}),
            )
            .await
            .unwrap();
        let final_clear = session
            .published_diagnostics_after(&uris[survivor], mark)
            .await
            .unwrap();
        assert!(
            final_clear["params"]["diagnostics"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        join_session(session, server).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn ineligible_collision_clears_all_actual_open_uris() {
    use std::os::unix::ffi::OsStringExt;
    use std::time::Duration;

    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ADOPT).unwrap();
    std::fs::create_dir(fixture.path("R")).unwrap();
    let source = "f <- function(x) {\n #| x integer\n x\n}\nf(\"bad\")\n";
    let uris = [b"bad\xff.R".as_slice(), b"bad\xfe.R".as_slice()].map(|name| {
        let path = fixture
            .path("R")
            .join(std::ffi::OsString::from_vec(name.to_vec()));
        std::fs::write(&path, source).unwrap();
        file_uri(&path).unwrap()
    });
    let invented = tower_lsp::lsp_types::Url::from_file_path(fixture.path("R/bad\u{fffd}.R"))
        .unwrap()
        .to_string();
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    session.open(&uris[0], 1, source).await.unwrap();
    let mark = session.publication_mark();
    session.open(&uris[1], 1, source).await.unwrap();
    let _ = session
        .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
        .await
        .unwrap();

    fixture
        .write_file(
            "ry.toml",
            "[index]\nmax-file-bytes = 1\n[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
        )
        .unwrap();
    let mark = session.publication_mark();
    session
        .notify("workspace/didChangeConfiguration", json!({"settings": {}}))
        .await
        .unwrap();
    let cleared = session
        .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
        .await
        .unwrap();
    for uri in &uris {
        assert!(cleared.get(uri).is_some_and(Vec::is_empty), "{cleared:?}");
    }
    assert!(!cleared.contains_key(&invented), "{cleared:?}");
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn mixed_content_collision_reports_only_the_uri_with_a_clause() {
    use std::time::Duration;

    let annotated = "f <- function(x) {\n #| x integer\n x\n}\nf(\"bad\")\n";
    let plain = format!(
        "{}missing_collision_name\n",
        annotated.replace("#| x integer", "# ordinary")
    );
    for first_annotated in [true, false] {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("ry.toml", ADOPT).unwrap();
        std::fs::create_dir(fixture.path("R")).unwrap();
        let sources = if first_annotated {
            [annotated, plain.as_str()]
        } else {
            [plain.as_str(), annotated]
        };
        let paths = colliding_paths(&fixture);
        for (path, source) in paths.iter().zip(sources) {
            std::fs::write(path, source).unwrap();
        }
        let uris = paths.map(|path| file_uri(&path).unwrap());
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        let mark = session.publication_mark();
        session.open(&uris[0], 1, sources[0]).await.unwrap();
        let _ = session
            .published_diagnostics_after(&uris[0], mark)
            .await
            .unwrap();
        let mark = session.publication_mark();
        session.open(&uris[1], 7, sources[1]).await.unwrap();
        let both = session
            .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
            .await
            .unwrap();
        for index in 0..2 {
            let expected =
                usize::from(index == 0 && first_annotated || index == 1 && !first_annotated);
            assert_eq!(
                both[&uris[index]]
                    .iter()
                    .filter(|diagnostic| diagnostic["code"] == "RY117")
                    .count(),
                expected,
                "{both:?}"
            );
        }
        assert!(
            !both[&uris[0]]
                .iter()
                .any(|diagnostic| diagnostic["code"] == "RY010"),
            "inactive buffer must not inherit the active buffer's ordinary finding: {both:?}"
        );
        if first_annotated {
            assert!(
                both[&uris[1]]
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == "RY010"),
                "active plain buffer should retain its own finding: {both:?}"
            );
        }

        // Change the active second buffer, then close it. The first
        // buffer's older text/version must become authoritative.
        let mark = session.publication_mark();
        session
            .change(&uris[1], 8, json!([{"text": annotated}]))
            .await
            .unwrap();
        let changed = session
            .published_diagnostics_after(&uris[1], mark)
            .await
            .unwrap();
        assert_eq!(count_code(&changed, "RY117"), 1, "{changed}");
        let mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uris[1]}}),
            )
            .await
            .unwrap();
        let survivor = session
            .published_diagnostics_after(&uris[0], mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&survivor, "RY117"),
            usize::from(first_annotated),
            "{survivor}"
        );
        let mark = session.publication_mark();
        session
            .change(&uris[0], 2, json!([{"text": annotated}]))
            .await
            .unwrap();
        let restored = session
            .published_diagnostics_after(&uris[0], mark)
            .await
            .unwrap();
        assert_eq!(count_code(&restored, "RY117"), 1, "{restored}");
        join_session(session, server).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn colliding_buffers_apply_size_cap_to_each_original_source() {
    use std::time::Duration;

    let annotated = "f <- function(x) {\n #| x integer\n x\n}\n";
    let plain = "z <- 1L\n";
    for active_oversized in [false, true] {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file(
                "ry.toml",
                "[index]\nmax-file-bytes = 100\n[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
            )
            .unwrap();
        std::fs::create_dir(fixture.path("R")).unwrap();
        let first = if active_oversized {
            annotated.to_string()
        } else {
            format!("{annotated}#{}\n", "x".repeat(200))
        };
        let second = if active_oversized {
            format!("{plain}#{}\n", "x".repeat(200))
        } else {
            plain.to_string()
        };
        let paths = colliding_paths(&fixture);
        for (path, text) in paths.iter().zip([&first, &second]) {
            std::fs::write(path, text).unwrap();
        }
        let uris = paths.map(|path| file_uri(&path).unwrap());
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        session.open(&uris[0], 1, &first).await.unwrap();
        let mark = session.publication_mark();
        session.open(&uris[1], 2, &second).await.unwrap();
        let both = session
            .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
            .await
            .unwrap();
        assert_eq!(
            both[&uris[0]]
                .iter()
                .filter(|d| d["code"] == "RY117")
                .count(),
            usize::from(active_oversized),
            "{both:?}"
        );
        assert!(!both[&uris[1]].iter().any(|d| d["code"] == "RY117"));

        let mark = session.publication_mark();
        session
            .change(&uris[1], 4, json!([{"text": annotated}]))
            .await
            .unwrap();
        let active_edit = session
            .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
            .await
            .unwrap();
        assert_eq!(
            active_edit[&uris[1]]
                .iter()
                .filter(|d| d["code"] == "RY117")
                .count(),
            1,
            "{active_edit:?}"
        );
        let mark = session.publication_mark();
        session
            .change(&uris[1], 5, json!([{"text": second}]))
            .await
            .unwrap();
        let active_restored = session
            .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
            .await
            .unwrap();
        assert!(
            !active_restored[&uris[1]]
                .iter()
                .any(|d| d["code"] == "RY117")
        );

        // An inactive edit changes only that URI's byte budget;
        // closing the active sibling restores the same snapshot.
        let mark = session.publication_mark();
        session
            .change(&uris[0], 3, json!([{"text": annotated}]))
            .await
            .unwrap();
        let edited = session
            .quiesce_diagnostics(&uris[0], mark, Duration::from_millis(200))
            .await
            .unwrap();
        assert_eq!(
            edited[&uris[0]]
                .iter()
                .filter(|d| d["code"] == "RY117")
                .count(),
            1,
            "{edited:?}"
        );

        let mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uris[1]}}),
            )
            .await
            .unwrap();
        let restored = session
            .published_diagnostics_after(&uris[0], mark)
            .await
            .unwrap();
        assert_eq!(count_code(&restored, "RY117"), 1, "{restored}");
        join_session(session, server).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn colliding_buffers_keep_independent_suppression_and_version() {
    use std::time::Duration;

    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ADOPT).unwrap();
    std::fs::create_dir(fixture.path("R")).unwrap();
    let plain = "f <- function(x) {\n #| x integer\n x\n}\n";
    let ignored = format!("# ry: ignore-file\n{plain}");
    let paths = colliding_paths(&fixture);
    for path in &paths {
        std::fs::write(path, plain).unwrap();
    }
    let uris = paths.map(|path| file_uri(&path).unwrap());
    let (mut session, server) = spawn_session(
        &[fixture.root()],
        json!({"textDocument": {"publishDiagnostics": {"dataSupport": true}}}),
        None,
    )
    .await;
    session.open(&uris[0], 1, &ignored).await.unwrap();
    let mark = session.publication_mark();
    session.open(&uris[1], 7, plain).await.unwrap();
    let both = session
        .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
        .await
        .unwrap();
    assert!(!both[&uris[0]].iter().any(|d| d["code"] == "RY117"));
    let second = both[&uris[1]]
        .iter()
        .find(|d| d["code"] == "RY117")
        .unwrap();
    assert_eq!(second["data"]["ry"]["version"], 7);

    let mark = session.publication_mark();
    session
        .change(&uris[1], 8, json!([{"text": ignored}]))
        .await
        .unwrap();
    let both = session
        .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
        .await
        .unwrap();
    assert!(!both[&uris[0]].iter().any(|d| d["code"] == "RY117"));
    assert!(!both[&uris[1]].iter().any(|d| d["code"] == "RY117"));

    let mark = session.publication_mark();
    session
        .change(&uris[0], 3, json!([{"text": plain}]))
        .await
        .unwrap();
    let both = session
        .quiesce_diagnostics(&uris[0], mark, Duration::from_millis(200))
        .await
        .unwrap();
    let first = both[&uris[0]]
        .iter()
        .find(|d| d["code"] == "RY117")
        .unwrap();
    assert_eq!(first["data"]["ry"]["version"], 3);
    assert!(!both[&uris[1]].iter().any(|d| d["code"] == "RY117"));
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn aborted_collision_edits_keep_active_uri_and_valid_prefix_still_commits() {
    use std::time::Duration;

    let fixture = FixtureProject::empty().unwrap();
    std::fs::create_dir(fixture.path("R")).unwrap();
    let paths = colliding_paths(&fixture);
    std::fs::write(&paths[0], "x <- 1L\n").unwrap();
    std::fs::write(&paths[1], "b <- genuinely_missing_name\n").unwrap();
    let uris = paths.map(|path| file_uri(&path).unwrap());
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    session.open(&uris[0], 1, "x <- 1L\n").await.unwrap();
    let mark = session.publication_mark();
    session
        .open(&uris[1], 1, "b <- genuinely_missing_name\n")
        .await
        .unwrap();
    let active = session
        .published_diagnostics_after(&uris[1], mark)
        .await
        .unwrap();
    assert_eq!(count_code(&active, "RY010"), 1, "{active}");

    let invalid = json!({"start": {"line": 99, "character": 0},
                         "end": {"line": 99, "character": 1}});
    for (version, batch) in [
        (2, json!([{"range": invalid, "text": "changed"}])),
        (3, json!([])),
    ] {
        sync_barrier(&mut session, &uris[1]).await;
        let mark = session.publication_mark();
        session.change(&uris[0], version, batch).await.unwrap();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(400),
                session.published_diagnostics_after(&uris[1], mark)
            )
            .await
            .is_err(),
            "inactive aborted edit cleared active diagnostics"
        );
    }

    let mark = session.publication_mark();
    session
        .change(
            &uris[0],
            4,
            json!([
                {"text": "a <- another_missing\n"},
                {"range": invalid, "text": "discarded"}
            ]),
        )
        .await
        .unwrap();
    let prefix = session
        .published_diagnostics_after(&uris[0], mark)
        .await
        .unwrap();
    assert_eq!(count_code(&prefix, "RY010"), 1, "{prefix}");
    assert!(prefix.to_string().contains("another_missing"), "{prefix}");

    let mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uris[0]}}),
        )
        .await
        .unwrap();
    let restored = session
        .published_diagnostics_after(&uris[1], mark)
        .await
        .unwrap();
    assert_eq!(count_code(&restored, "RY010"), 1, "{restored}");
    join_session(session, server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn colliding_sources_cannot_offer_other_buffers_quickfix() {
    use std::time::Duration;

    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", ADOPT).unwrap();
    std::fs::create_dir(fixture.path("R")).unwrap();
    let source = "f <- function(x) {\n #| x integer\n x\n}\n";
    let sources = [
        source.replace("f <-", "first_function_with_long_name <-"),
        source.replace("f <-", "g <-"),
    ];
    let paths = colliding_paths(&fixture);
    for (path, text) in paths.iter().zip(&sources) {
        std::fs::write(path, text).unwrap();
    }
    let uris = paths.map(|path| file_uri(&path).unwrap());
    let (mut session, server) = spawn_session(
        &[fixture.root()],
        json!({"workspace": {"workspaceEdit": {"documentChanges": true}},
               "textDocument": {"publishDiagnostics": {"dataSupport": true}}}),
        None,
    )
    .await;
    session.open(&uris[0], 1, &sources[0]).await.unwrap();
    let mark = session.publication_mark();
    session.open(&uris[1], 1, &sources[1]).await.unwrap();
    let both = session
        .quiesce_diagnostics(&uris[1], mark, Duration::from_millis(200))
        .await
        .unwrap();
    let first = both[&uris[0]]
        .iter()
        .find(|d| d["code"] == "RY117")
        .unwrap();
    let response = session
        .request(
            "textDocument/codeAction",
            json!({"textDocument": {"uri": uris[0]}, "range": first["range"],
                   "context": {"diagnostics": [first]}}),
        )
        .await
        .unwrap();
    assert!(response.is_null(), "{response}");

    let mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uris[1]}}),
        )
        .await
        .unwrap();
    let single = session
        .published_diagnostics_after(&uris[0], mark)
        .await
        .unwrap();
    let diag = single["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "RY117")
        .unwrap();
    let response = session
        .request(
            "textDocument/codeAction",
            json!({"textDocument": {"uri": uris[0]}, "range": diag["range"],
                   "context": {"diagnostics": [diag]}}),
        )
        .await
        .unwrap();
    assert!(
        response
            .as_array()
            .is_some_and(|actions| !actions.is_empty()),
        "{response}"
    );
    join_session(session, server).await;
}
