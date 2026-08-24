//! Workspace-root isolation for LSP requests.
//!
//! Requests that read open-document state must never resolve into an
//! unrelated workspace root: a same-named binding in a different root
//! must never win. The property was originally reported against
//! `goto_definition`, then carried by `signature_help`'s open-document
//! fallback (`eligible_open_documents`); that machinery was removed
//! with the completion/signatureHelp removal (issue #87), so the
//! property is now pinned on `textDocument/inlayHint` — the remaining
//! interactive request that serves answers from per-document cached
//! state (parse + single-file scope). A hint for a document in one
//! root must reflect that root's document only, even when another
//! root has an open document binding the same name to a different
//! type.

use ry_testkit::{AsyncJsonRpcClient, FixtureProject, file_uri};
use serde_json::{Value, json};

/// Run a future on a current-thread tokio runtime (same pattern as
/// session.rs / configuration_refresh.rs).
fn run<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(future)
}

// ════════════════════════════════════════════════════════════════════════════
// Requests must not cross workspace roots
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn inlay_hint_does_not_cross_workspace_roots() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // root-a binds `x` to an integer.
        fixture.write_file("root-a/R/a.R", "x <- 1L\n").unwrap();
        // root-b binds the SAME name to a character vector — in a
        // DIFFERENT root, so it must never win.
        fixture
            .write_file("root-b/R/b.R", "x <- \"leak\"\n")
            .unwrap();

        let root_a = fixture.path("root-a");
        let root_b = fixture.path("root-b");
        let root_uri = file_uri(fixture.root()).unwrap();
        let root_a_uri = file_uri(&root_a).unwrap();
        let root_b_uri = file_uri(&root_b).unwrap();

        let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
        let (client_reader, client_writer) = tokio::io::split(client_stream);
        let (server_reader, server_writer) = tokio::io::split(server_stream);
        let server = tokio::spawn(async move {
            let _ = ry_lsp::run_with(server_reader, server_writer).await;
        });
        let mut client = AsyncJsonRpcClient::new(client_reader, client_writer);

        let init_id = client
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "workspaceFolders": [
                        {"uri": root_a_uri, "name": "root-a"},
                        {"uri": root_b_uri, "name": "root-b"}
                    ]
                }),
            )
            .await
            .unwrap();
        client
            .receive_until(|m| m.get("id") == Some(&json!(init_id)), 16)
            .await
            .unwrap();
        client.notify("initialized", json!({})).await.unwrap();
        // Let the background indexer settle.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let a_uri = file_uri(&fixture.path("root-a/R/a.R")).unwrap();
        let b_uri = file_uri(&fixture.path("root-b/R/b.R")).unwrap();

        // Open both documents.
        client
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": a_uri, "languageId": "r", "version": 1, "text": "x <- 1L\n"
                }}),
            )
            .await
            .unwrap();
        client
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": b_uri,
                    "languageId": "r",
                    "version": 1,
                    "text": "x <- \"leak\"\n"
                }}),
            )
            .await
            .unwrap();

        // Request inlay hints for root-a's document. root-b's open
        // document binds the same name to a character vector; the hint
        // must still report root-a's own integer binding. Any hint
        // carrying root-b's type here means the request crossed roots.
        let hint_id = client
            .request(
                "textDocument/inlayHint",
                json!({
                    "textDocument": {"uri": a_uri},
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 1, "character": 0}
                    }
                }),
            )
            .await
            .unwrap();
        // Drop open/index diagnostic noise; keep the matching response.
        let response = client
            .receive_until(|m| m.get("id") == Some(&json!(hint_id)), 128)
            .await
            .unwrap();
        let result = response.get("result").cloned().unwrap_or(Value::Null);

        let hints = result
            .as_array()
            .expect("inlay hints for root-a's `x <- 1L` should be a non-null array");
        assert_eq!(hints.len(), 1, "one hint for `x`; got: {hints:?}");
        let label = hints[0]["label"].as_str().expect("hint label is a string");
        assert!(
            label.contains("integer"),
            "root-a's hint must show root-a's integer binding; got: {label}"
        );
        assert!(
            !label.contains("character"),
            "root-b's character binding leaked across roots; got: {label}"
        );

        // Best-effort teardown.
        let _ = client.notify("exit", Value::Null).await;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
    })
}
