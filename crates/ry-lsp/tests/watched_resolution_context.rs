//! #527: a file added through a watched event must resolve its
//! configured globals (and other resolution context) without a rescan.
//!
//! A watched create lands a fresh parse in `disk_files` but never advances
//! the per-file resolution maps (`external_bindings`, `bare_bindings`,
//! `imported_bindings`, `s3_methods`, `load_bindings`), which only the
//! full background scan rebuilt — the new file checked against an empty
//! context and disagreed with its neighbors and a fresh server.
//!
//! The test below pins the fixed contract: with a `globals`-configured
//! project settled and zero documents open, create a file using the
//! configured global, forward the watched event, and require a clean
//! publication for it. Without the fix the publication arrives (via the
//! #528 scheduling) but carries the spurious RY010.

mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session, sync_barrier};
use ry_testkit::{FixtureProject, rpc_receive_timeout};
use serde_json::{Value, json};

/// Capabilities advertising dynamic watched-file registration, so the
/// server registers its R source globs and the test client answers the
/// registration request.
fn watching_capabilities() -> Value {
    json!({"workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}})
}

/// Answer the server's `client/registerCapability` request (if any) so the
/// session proceeds; the globs themselves are pinned elsewhere. The wait
/// uses the shared receive budget (#551): the request is server work that
/// parallel test load can starve past a fixed 5s.
async fn answer_watcher_registration(session: &mut harness::ClientSession) {
    let _: Option<Result<Value, _>> = tokio::time::timeout(
        rpc_receive_timeout(),
        session.respond_to_request("client/registerCapability", json!(null)),
    )
    .await
    .ok()
    .map(|result| result.map_err(|_| ()));
}

fn has_ry010_for(publish: &Value, name: &str) -> bool {
    normalize_diagnostics(publish).iter().any(|diagnostic| {
        diagnostic["code"] == json!("RY010")
            && diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains(name))
    })
}

/// Settle the session so the initial background index has committed:
/// open `main.R` and await its publication (the publish path returns
/// early while `initial_index_pending` holds, so the publication proves
/// the flag cleared), then close it again for the zero-open-docs shape
/// both issues exercise.
async fn settle_then_close(session: &mut harness::ClientSession, main_uri: &str) {
    session.open(main_uri, 1, "w <- 1L\n").await.unwrap();
    session
        .published_diagnostics_after(main_uri, session.publication_mark())
        .await
        .unwrap();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": main_uri}}),
        )
        .await
        .unwrap();
    sync_barrier(session, main_uri).await;
}

#[test]
fn watched_create_resolves_configured_globals_without_a_rescan() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "globals = [\"my_global\"]\n")
            .unwrap();
        fixture.write_file("main.R", "w <- 1L\n").unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        answer_watcher_registration(&mut session).await;
        sync_barrier(&mut session, &main_uri).await;
        settle_then_close(&mut session, &main_uri).await;

        // Create a closed file using the configured global and forward
        // the watched creation event with zero documents open.
        fixture.write_file("new.R", "x <- my_global\n").unwrap();
        let new_uri = file_uri(&fixture.path("new.R"));
        let create_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": new_uri, "type": 1}]}),
            )
            .await
            .unwrap();
        let publish = session
            .published_diagnostics_after(&new_uri, create_mark)
            .await
            .unwrap();
        assert!(
            !has_ry010_for(&publish, "my_global"),
            "the watched addition must resolve the configured global without a rescan: {publish}"
        );

        join_session(session, server).await;
    });
}

/// The inverse span-keyed case: moving a `load()` call to a different
/// line must move its bindings, not leave the old span entries behind.
/// `ordinary.rda` carries one object (`a`, verified in R); a use of `a`
/// before the `load()` line must report RY010 while the use after it
/// stays quiet — through a watched edit with zero documents open, where
/// the parse refreshes but only the group re-resolution advances the
/// span-keyed entries.
#[test]
fn watched_edit_moves_load_bindings_to_the_new_line() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("main.R", "w <- 1L\n").unwrap();
        let main_uri = file_uri(&fixture.path("main.R"));
        let (mut session, server) =
            spawn_session(&[fixture.root()], watching_capabilities(), None).await;
        answer_watcher_registration(&mut session).await;
        sync_barrier(&mut session, &main_uri).await;
        settle_then_close(&mut session, &main_uri).await;

        std::fs::write(
            fixture.path("data.rda"),
            include_bytes!("../../ry-testkit/testdata/complete-package/data/ordinary.rda"),
        )
        .unwrap();
        // `a` used on line 0 (before the load) fires; `a` on line 2
        // (after) is quiet.
        fixture
            .write_file("loader.R", "a\nload(\"data.rda\")\na\n")
            .unwrap();
        let loader_uri = file_uri(&fixture.path("loader.R"));
        let create_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": loader_uri, "type": 1}]}),
            )
            .await
            .unwrap();
        let created = session
            .published_diagnostics_after(&loader_uri, create_mark)
            .await
            .unwrap();
        let diags = normalize_diagnostics(&created);
        let line_of = |diagnostic: &Value| diagnostic["range"]["start"]["line"].as_u64().unwrap();
        assert!(
            diags
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("RY010")
                    && diagnostic["message"]
                        .as_str()
                        .is_some_and(|message| message.contains("`a`"))
                    && line_of(diagnostic) == 0),
            "the use of `a` before the load() line must report RY010: {created}"
        );
        assert!(
            !diags
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("RY010") && line_of(diagnostic) == 2),
            "the use of `a` after the load() line must resolve: {created}"
        );

        // Move the `load()` call above both uses: every `a` resolves.
        std::fs::write(fixture.path("loader.R"), "load(\"data.rda\")\na\na\n").unwrap();
        let edit_mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": loader_uri, "type": 2}]}),
            )
            .await
            .unwrap();
        let after = session
            .published_diagnostics_after(&loader_uri, edit_mark)
            .await
            .unwrap();
        assert!(
            normalize_diagnostics(&after)
                .iter()
                .all(|diagnostic| diagnostic["code"] != json!("RY010")),
            "moving load() above the uses must clear the stale span entry: {after}"
        );

        join_session(session, server).await;
    });
}
