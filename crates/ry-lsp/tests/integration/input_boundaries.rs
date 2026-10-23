use crate::harness;

use harness::{
    change_published, cli_diagnostics, count_code, file_uri, join_session, normalize_diagnostics,
    published_from_lsp, sorted_diagnostics, spawn_session, sync_barrier,
};
use ry_lsp::test_seam::Seam;
use ry_testkit::FixtureProject;
use serde_json::json;

#[tokio::test]
async fn first_diagnostics_wait_for_initial_workspace_bindings() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file(
            "ry.toml",
            "[[environments]]\nname = 'host'\nbindings = ['profile_binding']\npaths = ['main.R']\n",
        )
        .unwrap();
    fixture.write_file("main.R", "profile_binding\n").unwrap();
    let uri = file_uri(&fixture.path("main.R"));
    ry_lsp::test_seam::arm(Seam::Scan);
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    ry_lsp::test_seam::wait_arrived(Seam::Scan).await;
    // The document opens while the initial scan is parked before its
    // install; its publication must wait for the installed bindings.
    let mark = session.publication_mark();
    session.open(&uri, 1, "profile_binding\n").await.unwrap();
    sync_barrier(&mut session, &uri).await;
    ry_lsp::test_seam::release(Seam::Scan);
    let first = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert!(
        !normalize_diagnostics(&first)
            .iter()
            .any(|d| d["code"] == "RY010"),
        "the first publication must use the configured host bindings: {first}"
    );
    join_session(session, server).await;
}

#[tokio::test]
async fn removing_all_folders_during_initial_index_releases_diagnostics() {
    let fixture = FixtureProject::empty().unwrap();
    let outside = FixtureProject::empty().unwrap();
    let source = "1L + 'bad'\n";
    outside.write_file("main.R", source).unwrap();
    let uri = file_uri(&outside.path("main.R"));
    ry_lsp::test_seam::arm(Seam::Scan);
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    ry_lsp::test_seam::wait_arrived(Seam::Scan).await;
    let mark = session.publication_mark();
    session.open(&uri, 1, source).await.unwrap();
    session
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({"event": {"added": [], "removed": [{
                "uri": file_uri(fixture.root()), "name": "removed"
            }]}}),
        )
        .await
        .unwrap();
    // The removal queues behind the parked initial scan. Once it runs, no
    // folder is left to index, and the outside document must become
    // checkable.
    ry_lsp::test_seam::release(Seam::Scan);
    let publish = session
        .published_diagnostics_after(&uri, mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&publish)
            .iter()
            .any(|d| d["code"] == "RY040")
    );
    join_session(session, server).await;
}

/// An unopened sibling that is not clean UTF-8 is analyzed from its
/// decoded disk bytes with an RY000 encoding flag, as in `ry check`.
/// Opening it with client-decoded text clears the flag, and opening or
/// closing it leaves the diagnostics of the file that uses it unchanged.
#[tokio::test]
async fn non_utf8_siblings_are_flagged_and_survive_open_and_close() {
    let source = "value <- make_label() + 1\n";
    // (helper bytes on disk, helper text the client decodes, whether
    // `main.R` reports RY040)
    let cases: [(&[u8], &str, bool); 2] = [
        (
            b"make_label <- function() \"caf\xe9\"\n",
            "make_label <- function() \"café\"\n",
            true,
        ),
        (
            b"\xef\xbb\xbfmake_label <- function() 1\n",
            "make_label <- function() 1\n",
            false,
        ),
    ];
    for (bytes, decoded, main_ry040) in cases {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("main.R", source).unwrap();
        fixture.write_file("helper.R", bytes).unwrap();
        assert_eq!(
            cli_diagnostics(fixture.root(), ".", &[])
                .iter()
                .any(|d| d.path == "main.R" && d.code == "RY040"),
            main_ry040,
            "{decoded}"
        );
        assert!(
            cli_diagnostics(fixture.root(), "helper.R", &[])
                .iter()
                .any(|d| d.code == "RY000"),
            "the CLI must flag the helper: {decoded}"
        );
        let main_uri = file_uri(&fixture.path("main.R"));
        let helper_uri = file_uri(&fixture.path("helper.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        let mark = session.publication_mark();
        session.open(&main_uri, 1, source).await.unwrap();
        let published = session
            .quiesce_diagnostics(&main_uri, mark, std::time::Duration::from_millis(60))
            .await
            .unwrap();
        let expected = sorted_diagnostics(&published[&main_uri]);
        assert_eq!(
            expected.iter().any(|d| d["code"] == "RY040"),
            main_ry040,
            "{expected:?}"
        );
        let helper = published.get(&helper_uri).map(Vec::as_slice).unwrap_or(&[]);
        assert!(
            helper.iter().any(|d| d["code"] == "RY000"),
            "the unopened helper must carry the encoding flag: {helper:?}"
        );

        let mark = session.publication_mark();
        session.open(&helper_uri, 1, decoded).await.unwrap();
        let republished = session
            .quiesce_diagnostics(&helper_uri, mark, std::time::Duration::from_millis(60))
            .await
            .unwrap();
        let helper = republished
            .get(&helper_uri)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        assert!(
            helper.iter().all(|d| d["code"] != "RY000"),
            "an opened buffer is client-decoded text and carries no disk flag: {helper:?}"
        );
        sync_barrier(&mut session, &main_uri).await;
        let reopened = change_published(&mut session, &main_uri, 2, source).await;
        assert_eq!(normalize_diagnostics(&reopened), expected);

        session.close(&helper_uri).await.unwrap();
        sync_barrier(&mut session, &main_uri).await;
        let closed = change_published(&mut session, &main_uri, 3, source).await;
        assert_eq!(normalize_diagnostics(&closed), expected);
        join_session(session, server).await;
    }
}

#[tokio::test]
async fn environment_paths_are_anchored_globs_in_both_frontends() {
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
        let findings = cli_diagnostics(fixture.root(), path, &[]);
        assert_eq!(
            findings.iter().any(|d| d.code == "RY010"),
            expected_unbound,
            "{path}: {findings:?}"
        );
        let uri = file_uri(&fixture.path(path));
        let publish = harness::open_published(&mut session, &uri, 1, "profile_binding\n").await;
        assert_eq!(
            normalize_diagnostics(&publish)
                .iter()
                .any(|d| d["code"] == "RY010"),
            expected_unbound,
            "{path}: {publish}"
        );
    }
    join_session(session, server).await;
}

/// An open buffer ry cannot parse at all (here one call nested past the
/// syntax nesting limit of 128) publishes the one RY000 `ry check` reports
/// for it, at the same position, in place of its earlier findings. The
/// RY000 clears once the buffer parses again (#785).
#[tokio::test]
async fn unparseable_open_buffer_publishes_the_cli_ry000_until_it_parses() {
    let deep = format!("{}1{}\n", "f(".repeat(512), ")".repeat(512));
    let fixture = FixtureProject::empty().unwrap();
    let path = fixture.write_file("deep.R", &deep).unwrap();
    let cli = cli_diagnostics(fixture.root(), "deep.R", &[]);
    assert!(
        cli.len() == 1 && cli[0].code == "RY000" && cli[0].byte_column > 0,
        "{cli:?}"
    );
    let uri = file_uri(&path);
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &uri).await;

    let unbound = "x <- never_bound_here\n";
    let publish = harness::open_published(&mut session, &uri, 1, unbound).await;
    assert_eq!(count_code(&publish, "RY010"), 1, "{publish}");

    let publish = change_published(&mut session, &uri, 2, &deep).await;
    assert_eq!(
        published_from_lsp(&publish, &path, fixture.root()),
        cli,
        "{publish}"
    );

    let publish = change_published(&mut session, &uri, 3, "x <- 1\n").await;
    assert!(normalize_diagnostics(&publish).is_empty(), "{publish}");
    join_session(session, server).await;
}
