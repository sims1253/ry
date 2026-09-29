//! Adopted source annotations use the same warm checking path in the editor.

mod harness;

use harness::{join_session, spawn_session, sync_barrier};
use ry_testkit::{FixtureProject, file_uri};
use serde_json::{Value, json};

fn count_code(publish: &Value, code: &str) -> usize {
    publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["code"] == code)
        .count()
}

#[test]
fn annotation_only_editor_edits_retract_and_restore_mismatch() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file(
                    "ry.toml",
                    "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
                )
                .unwrap();
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
        });
}

#[cfg(unix)]
#[test]
fn native_filename_and_unicode_collision_never_adopt_the_wrong_contract() {
    use std::os::unix::ffi::OsStringExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file(
                    "ry.toml",
                    "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
                )
                .unwrap();
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
        });
}

#[cfg(unix)]
#[test]
fn sole_genuine_unicode_replacement_filename_adopts_normally() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file(
                    "ry.toml",
                    "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
                )
                .unwrap();
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
        });
}
