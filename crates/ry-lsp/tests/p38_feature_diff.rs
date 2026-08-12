//! P38-W1: Feature differential — deterministic red cases exposing the
//! central architecture defect documented in Plan 38 findings B2–B5.
//!
//! These tests are `#[ignore]`'d because they verify project-aware semantic
//! behavior that does not yet exist. Each test's doc-comment names the finding
//! and the P38 workstream that will make it pass.
//!
//! ## Findings exposed
//!
//! - **B2**: hover, completion, inlay hints, and symbols are single-file only;
//!   they do not see definitions in sibling files.
//! - **B3**: definition, references, rename, and highlights are syntax-name
//!   based, not resolved-symbol based.
//! - **B4**: references and rename omit unopened (disk-only) files.
//! - **B5**: signature help is incomplete for user-defined project functions.

use ry_testkit::{FixtureProject, LspSession, file_uri};
use serde_json::{Value, json};
use std::path::Path;

/// Position helper: line and character (both 0-based, UTF-16 code units).
fn pos(line: u32, character: u32) -> Value {
    json!({"line": line, "character": character})
}

type Session = LspSession<
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
>;

/// Spawn an LSP server and return a connected session.
async fn spawn_session(root: &Path) -> (Session, tokio::task::JoinHandle<()>) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move {
        let _ = ry_lsp::run_with(server_reader, server_writer).await;
    });
    let mut session = LspSession::new(client_reader, client_writer);
    session.initialize(root).await.unwrap();
    // Wait for background indexing to populate disk_files.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    (session, server)
}

/// Run a future on a current-thread tokio runtime (same pattern as session.rs).
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
// B2: Hover/completion are single-file (P38-W7 will fix)
// ════════════════════════════════════════════════════════════════════════════

/// A function defined in an unopened sibling file should provide hover type
/// information in the consuming file. Currently it returns null because the
/// hover checker only sees the single open file.
#[test]
fn b2_hover_for_project_function_from_sibling() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("R/defs.R", "project_fn <- function(x) x * 2\n")
            .unwrap();
        fixture.write_file("R/use.R", "project_fn(1)\n").unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let use_uri = file_uri(&fixture.path("R/use.R")).unwrap();
        session.open(&use_uri, 1, "project_fn(1)\n").await.unwrap();

        let hover = session
            .request(
                "textDocument/hover",
                json!({
                    "textDocument": {"uri": use_uri},
                    "position": pos(0, 1)
                }),
            )
            .await
            .unwrap();

        let contents = hover.get("contents");
        assert!(
            contents.is_some() && contents != Some(&Value::Null),
            "hover should return type info for project function defined in sibling file"
        );
    })
}

/// Completion should include functions defined in project files.
#[test]
fn b2_completion_includes_project_function() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("R/defs.R", "unique_project_fn <- function(x) x\n")
            .unwrap();
        fixture.write_file("R/use.R", "unique_\n").unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let use_uri = file_uri(&fixture.path("R/use.R")).unwrap();
        session.open(&use_uri, 1, "unique_\n").await.unwrap();

        let completion = session
            .request(
                "textDocument/completion",
                json!({
                    "textDocument": {"uri": use_uri},
                    "position": pos(0, 7)
                }),
            )
            .await
            .unwrap();

        let empty = Vec::new();
        let items = completion.as_array().unwrap_or(&empty);
        let has_project_fn = items.iter().any(|item| {
            item.get("label")
                .and_then(|l| l.as_str())
                .map(|l| l == "unique_project_fn")
                .unwrap_or(false)
        });
        assert!(
            has_project_fn,
            "completion should include project function defined in sibling file"
        );
    })
}

// ════════════════════════════════════════════════════════════════════════════
// B3: Navigation uses syntax-name matching (P38-W6 will fix)
// ════════════════════════════════════════════════════════════════════════════

/// Go-to-definition should resolve to the correct definition based on scope,
/// not just the first syntactic match of the same name string.
#[test]
fn b3_definition_uses_resolved_symbol_not_spelling() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("R/a.R", "helper <- function() 1\n")
            .unwrap();
        fixture
            .write_file("R/b.R", "helper <- function() 2\nhelper()\n")
            .unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let b_uri = file_uri(&fixture.path("R/b.R")).unwrap();
        let a_uri = file_uri(&fixture.path("R/a.R")).unwrap();
        session
            .open(&b_uri, 1, "helper <- function() 2\nhelper()\n")
            .await
            .unwrap();

        let def = session
            .request(
                "textDocument/definition",
                json!({
                    "textDocument": {"uri": b_uri},
                    "position": pos(1, 1)
                }),
            )
            .await
            .unwrap();

        let locations = match &def {
            Value::Array(arr) => arr.clone(),
            Value::Object(obj) => vec![Value::Object(obj.clone())],
            _ => Vec::new(),
        };

        let target_uris: Vec<&str> = locations
            .iter()
            .filter_map(|l| l.get("uri").and_then(|u| u.as_str()))
            .collect();

        assert!(
            target_uris.iter().all(|uri| *uri != a_uri),
            "definition should resolve to the local b.R helper, not a.R with the same spelling"
        );
    })
}

// ════════════════════════════════════════════════════════════════════════════
// B4: References/rename omit unopened files (P38-W6 will fix)
// ════════════════════════════════════════════════════════════════════════════

/// References should include occurrences in unopened disk files.
#[test]
fn b4_references_include_unopened_files() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("R/define.R", "shared_var <- 1\n")
            .unwrap();
        fixture.write_file("R/use1.R", "shared_var + 1\n").unwrap();
        fixture.write_file("R/use2.R", "shared_var + 2\n").unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let define_uri = file_uri(&fixture.path("R/define.R")).unwrap();
        session
            .open(&define_uri, 1, "shared_var <- 1\n")
            .await
            .unwrap();

        let refs = session
            .request(
                "textDocument/references",
                json!({
                    "textDocument": {"uri": define_uri},
                    "position": pos(0, 1),
                    "context": {"includeDeclaration": true}
                }),
            )
            .await
            .unwrap();

        let ref_count = match &refs {
            Value::Array(arr) => arr.len(),
            Value::Null => 0,
            _ => 1,
        };
        assert!(
            ref_count >= 3,
            "references should include unopened disk files; found {} (expected >= 3)",
            ref_count
        );
    })
}

/// Rename should edit all occurrences across the project.
#[test]
fn b4_rename_edits_unopened_files() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("R/define.R", "old_name <- 1\n").unwrap();
        fixture.write_file("R/use.R", "old_name + 1\n").unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let define_uri = file_uri(&fixture.path("R/define.R")).unwrap();
        session
            .open(&define_uri, 1, "old_name <- 1\n")
            .await
            .unwrap();

        let use_uri = file_uri(&fixture.path("R/use.R")).unwrap();

        let rename = session
            .request(
                "textDocument/rename",
                json!({
                    "textDocument": {"uri": define_uri},
                    "position": pos(0, 1),
                    "newName": "new_name"
                }),
            )
            .await
            .unwrap();

        let changes = rename.get("changes");
        assert!(changes.is_some(), "rename should return changes");

        let has_use_r = changes
            .and_then(|c| c.as_object())
            .map(|obj| obj.contains_key(&use_uri))
            .unwrap_or(false);

        assert!(
            has_use_r,
            "rename should edit unopened files; use.R should be in changes"
        );
    })
}

// ════════════════════════════════════════════════════════════════════════════
// B5: Signature help for user-defined functions (P38-W7 will fix)
// ════════════════════════════════════════════════════════════════════════════

/// Signature help should show parameter info for user-defined project functions.
#[test]
fn b5_signature_help_for_user_defined_function() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file(
                "R/defs.R",
                "compute <- function(data, method, iterations) data\n",
            )
            .unwrap();
        fixture
            .write_file("R/call.R", "compute(1, 2, 3)\n")
            .unwrap();

        let (mut session, _server) = spawn_session(fixture.root()).await;

        let call_uri = file_uri(&fixture.path("R/call.R")).unwrap();
        session
            .open(&call_uri, 1, "compute(1, 2, 3)\n")
            .await
            .unwrap();

        let sig = session
            .request(
                "textDocument/signatureHelp",
                json!({
                    "textDocument": {"uri": call_uri},
                    "position": pos(0, 10)
                }),
            )
            .await
            .unwrap();

        let signatures = sig.get("signatures");
        assert!(
            signatures.is_some() && signatures != Some(&Value::Null),
            "signature help should provide parameter info for user-defined project functions"
        );

        let empty_sigs = Vec::new();
        let sig_array = signatures.and_then(|s| s.as_array()).unwrap_or(&empty_sigs);
        assert!(
            !sig_array.is_empty(),
            "signature help should return at least one signature"
        );

        let label = sig_array[0]
            .get("label")
            .and_then(|l| l.as_str())
            .unwrap_or("");
        assert!(
            label.contains("data") || label.contains("method") || label.contains("iterations"),
            "signature label should include parameter names; got: {}",
            label
        );
    })
}
