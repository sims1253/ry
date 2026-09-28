mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::json;

#[test]
fn unused_ignore_publication_tracks_fixes_moves_and_removal() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file("ry.toml", "warn = [\"RY113\"]\n")
                .unwrap();
            let initial = "1L == NA # ry: ignore[RY034]\n";
            let path = fixture.write_file("example.R", initial).unwrap();
            let uri = file_uri(&path);
            let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;

            let mark = session.publication_mark();
            session.open(&uri, 1, initial).await.unwrap();
            let published = session
                .published_diagnostics_after(&uri, mark)
                .await
                .unwrap();
            assert!(normalize_diagnostics(&published).is_empty(), "{published}");

            for (version, source, expected_line) in [
                (2, "1L == 1L # ry: ignore[RY034]\n", Some(0)),
                (3, "# heading\n# ry: ignore[RY034]\n1L == 1L\n", Some(1)),
                (4, "1L == 1L\n", None),
            ] {
                let mark = session.publication_mark();
                session
                    .change(&uri, version, json!([{"text":source}]))
                    .await
                    .unwrap();
                let published = session
                    .published_diagnostics_after(&uri, mark)
                    .await
                    .unwrap();
                let findings = normalize_diagnostics(&published);
                let audits: Vec<_> = findings.iter().filter(|d| d["code"] == "RY113").collect();
                assert_eq!(
                    audits.len(),
                    usize::from(expected_line.is_some()),
                    "{source}: {published}"
                );
                if let Some(line) = expected_line {
                    assert_eq!(audits[0]["range"]["start"]["line"], line);
                }
            }
            join_session(session, server).await;
        });
}
