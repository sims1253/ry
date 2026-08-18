//! Open-document fallback isolation for `signature_help`.
//!
//! The handler's cross-document fallback shares the
//! `eligible_open_documents` rule (current document first, then
//! same-folder-root candidates, sorted). These tests pin the two bugs
//! that rule fixes:
//!
//! * the fallback could resolve into an unrelated workspace root (a
//!   same-named function in a different root won, which is never
//!   correct) — originally reported against `goto_definition`, whose
//!   removal (issue #87) left `signature_help` as the rule's only
//!   consumer;
//! * `signature_help` sorted every open path and returned the first match,
//!   so a duplicated user-defined function name resolved to an unrelated
//!   file's parameters.

use ry_testkit::{AsyncJsonRpcClient, FixtureProject, LspSession, file_uri};
use serde_json::{Value, json};
use std::path::Path;

type Session = LspSession<
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
>;

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

/// Spawn an LSP server and return a connected session: initialize, then
/// briefly sleep so the background indexer settles before the test drives
/// the server.
async fn spawn_session(root: &Path) -> (Session, tokio::task::JoinHandle<()>) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move {
        let _ = ry_lsp::run_with(server_reader, server_writer).await;
    });
    let mut session = LspSession::new(client_reader, client_writer);
    session.initialize(root).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    (session, server)
}

// ════════════════════════════════════════════════════════════════════════════
// Item 2 — signature_help must not cross workspace roots
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn signature_help_does_not_cross_workspace_roots() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // root-a calls `bar` but never defines it.
        fixture.write_file("root-a/R/a.R", "bar(1)\n").unwrap();
        // root-b defines `bar` — in a DIFFERENT root, so it must never win.
        fixture
            .write_file("root-b/R/b.R", "bar <- function(leaked) {}\n")
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
                    "uri": a_uri, "languageId": "r", "version": 1, "text": "bar(1)\n"
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
                    "text": "bar <- function(leaked) {}\n"
                }}),
            )
            .await
            .unwrap();

        // Request signature help inside the call in root-a. root-a defines
        // no `bar`; the only definer is in root-b. The fallback must NOT
        // cross roots, so no signature may be served. The pre-fix fallback
        // iterated every open document and served root-b's parameters.
        let sig_id = client
            .request(
                "textDocument/signatureHelp",
                json!({
                    "textDocument": {"uri": a_uri},
                    "position": {"line": 0, "character": 4}
                }),
            )
            .await
            .unwrap();
        // Drop open/index diagnostic noise; keep the matching response.
        let response = client
            .receive_until(|m| m.get("id") == Some(&json!(sig_id)), 128)
            .await
            .unwrap();
        let result = response.get("result").cloned().unwrap_or(Value::Null);

        let leaked = result
            .pointer("/signatures/0/label")
            .and_then(Value::as_str)
            .is_some_and(|label| label.contains("leaked"));
        assert!(
            !leaked,
            "signature help must not cross into root-b; got: {result}"
        );
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(
            !serialized.contains(&b_uri),
            "signature help must not resolve to root-b's document; got: {serialized}"
        );

        // Best-effort teardown.
        let _ = client.notify("exit", Value::Null).await;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
    })
}

// ════════════════════════════════════════════════════════════════════════════
// Item 3 — signature_help must prefer the current document
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn signature_help_prefers_current_document() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // A definer that sorts BEFORE the current document, so the pre-fix
        // "sort every open path, take the first match" fallback picked it
        // even though the cursor was in a different file.
        fixture
            .write_file("aaa_first.R", "myfn <- function(leaked_param) {}\n")
            .unwrap();
        // The current document: also defines myfn, plus the call to position in.
        fixture
            .write_file(
                "zzz_current.R",
                "myfn <- function(current_param) {}\nmyfn()\n",
            )
            .unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let first_uri = file_uri(&fixture.path("aaa_first.R")).unwrap();
        let current_uri = file_uri(&fixture.path("zzz_current.R")).unwrap();

        session
            .open(&first_uri, 1, "myfn <- function(leaked_param) {}\n")
            .await
            .unwrap();
        session
            .open(
                &current_uri,
                1,
                "myfn <- function(current_param) {}\nmyfn()\n",
            )
            .await
            .unwrap();

        // Signature help inside the call on line 1 of the current document:
        // "myfn()" at character 5 sits right after the `(`.
        let signature = session
            .request(
                "textDocument/signatureHelp",
                json!({
                    "textDocument": {"uri": current_uri},
                    "position": {"line": 1, "character": 5}
                }),
            )
            .await
            .unwrap();

        // The current document defines myfn(current_param); the pre-fix
        // fallback sorted every open path and returned aaa_first.R's
        // myfn(leaked_param). The fix prefers the current document.
        assert_eq!(
            signature["signatures"][0]["label"], "myfn(current_param)",
            "signature help should use the current document's parameters; got: {signature}"
        );
    })
}
