use ry_testkit::{FixtureProject, LspSession, file_uri};
use serde_json::{Value, json};

const SOURCE: &str = concat!(
    "ascii <- 1L\r\n",
    "frame <- data.frame(column = 1L)\r\n",
    "marker <- \"é e\u{301} 😀\"; target <- 1L\r\n",
    "marker; target\r\n",
    "marker <- \"😀\"; frame$\r\n",
    "marker <- \"é😀\"; round(1L, 2L)\r\n",
    "marker <- \"😀\"; length(xx = 1L)\r\n",
    "`😀` <- 4L\r\n",
    "`😀`\r\n",
);
const OTHER_SOURCE: &str = "\"😀\"; target\r\n";
const DISK_SOURCE: &str = "marker <- \"😀\"; length(xx = 1L)\r\n";

#[derive(Clone, Copy, Debug)]
struct Anchor {
    name: &'static str,
    byte: usize,
    line: u32,
    scalar: u32,
    character: u32,
    following: &'static str,
}

// Hand-declared from the literal above. These values intentionally do not use
// ry-lsp or ry-testkit conversion helpers: byte offsets count UTF-8 bytes and
// character columns count UTF-16 code units.
const ANCHORS: &[Anchor] = &[
    Anchor {
        name: "ascii",
        byte: 0,
        line: 0,
        scalar: 0,
        character: 0,
        following: "ascii",
    },
    Anchor {
        name: "line after CRLF",
        byte: 13,
        line: 1,
        scalar: 0,
        character: 0,
        following: "frame",
    },
    Anchor {
        name: "BMP start",
        byte: 58,
        line: 2,
        scalar: 11,
        character: 11,
        following: "é",
    },
    Anchor {
        name: "BMP end",
        byte: 60,
        line: 2,
        scalar: 12,
        character: 12,
        following: " ",
    },
    Anchor {
        name: "combining base",
        byte: 61,
        line: 2,
        scalar: 13,
        character: 13,
        following: "e",
    },
    Anchor {
        name: "combining mark",
        byte: 62,
        line: 2,
        scalar: 14,
        character: 14,
        following: "\u{301}",
    },
    Anchor {
        name: "combining end",
        byte: 64,
        line: 2,
        scalar: 15,
        character: 15,
        following: " ",
    },
    Anchor {
        name: "astral start",
        byte: 65,
        line: 2,
        scalar: 16,
        character: 16,
        following: "😀",
    },
    Anchor {
        name: "astral end",
        byte: 69,
        line: 2,
        scalar: 17,
        character: 18,
        following: "\"",
    },
    Anchor {
        name: "target declaration",
        byte: 72,
        line: 2,
        scalar: 20,
        character: 21,
        following: "target",
    },
    Anchor {
        name: "target declaration end",
        byte: 78,
        line: 2,
        scalar: 26,
        character: 27,
        following: " ",
    },
    Anchor {
        name: "target read",
        byte: 94,
        line: 3,
        scalar: 8,
        character: 8,
        following: "target",
    },
    Anchor {
        name: "target read end",
        byte: 100,
        line: 3,
        scalar: 14,
        character: 14,
        following: "\r\n",
    },
    Anchor {
        name: "completion cursor",
        byte: 126,
        line: 4,
        scalar: 21,
        character: 22,
        following: "\r\n",
    },
    Anchor {
        name: "signature astral",
        byte: 141,
        line: 5,
        scalar: 12,
        character: 12,
        following: "😀",
    },
    Anchor {
        name: "signature cursor",
        byte: 158,
        line: 5,
        scalar: 26,
        character: 27,
        following: "2L",
    },
    Anchor {
        name: "diagnostic name",
        byte: 188,
        line: 6,
        scalar: 22,
        character: 23,
        following: "xx",
    },
    Anchor {
        name: "diagnostic name end",
        byte: 190,
        line: 6,
        scalar: 24,
        character: 25,
        following: " =",
    },
    Anchor {
        name: "diagnostic end",
        byte: 195,
        line: 6,
        scalar: 29,
        character: 30,
        following: ")",
    },
    Anchor {
        name: "backtick astral",
        byte: 199,
        line: 7,
        scalar: 1,
        character: 1,
        following: "😀",
    },
    Anchor {
        name: "backtick astral end",
        byte: 203,
        line: 7,
        scalar: 2,
        character: 3,
        following: "`",
    },
    Anchor {
        name: "multiline backtick read",
        byte: 213,
        line: 8,
        scalar: 1,
        character: 1,
        following: "😀",
    },
];

fn anchor(name: &str) -> Anchor {
    *ANCHORS.iter().find(|anchor| anchor.name == name).unwrap()
}

fn position(name: &str) -> Value {
    let anchor = anchor(name);
    json!({"line": anchor.line, "character": anchor.character})
}

fn assert_position(actual: &Value, name: &str) {
    assert_eq!(actual, &position(name), "wrong LSP position for {name}");
}

/// Request inlay hints for a single line of `uri`. The transcript test
/// uses this as its state-observable probe: a response is produced from
/// the cached parse and the checked scope, so it reflects exactly what
/// the server currently believes the document contains.
async fn hints_for_line(session: &mut ClientSession, uri: &str, line: u32) -> Value {
    session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": line, "character": 0},
                    "end": {"line": line + 1, "character": 0}
                }
            }),
        )
        .await
        .unwrap()
}

/// Assert the server placed a type hint immediately after the binding
/// identifier at (`line`, `character`).
fn assert_hint_at(hints: &Value, line: u32, character: u32) {
    assert!(
        hints
            .as_array()
            .expect("inlay hints should be an array")
            .iter()
            .any(
                |hint| hint["position"] == json!({"line": line, "character": character})
                    && hint["label"]
                        .as_str()
                        .is_some_and(|label| label.starts_with(": "))
            ),
        "expected a type hint at ({line}, {character}); got: {hints}"
    );
}

#[test]
fn utf16_contract_holds_across_one_real_lsp_transcript() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(utf16_transcript());
}

async fn utf16_transcript() {
    for anchor in ANCHORS {
        assert!(
            SOURCE.as_bytes()[anchor.byte..].starts_with(anchor.following.as_bytes()),
            "bad independent byte offset for {}",
            anchor.name
        );
        let prefix = &SOURCE[..anchor.byte];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
        let line_start = prefix.rfind('\n').map_or(0, |byte| byte + 1);
        let line_prefix = &SOURCE[line_start..anchor.byte];
        assert_eq!(line, anchor.line, "bad line for {}", anchor.name);
        assert_eq!(
            line_prefix.chars().count() as u32,
            anchor.scalar,
            "bad Unicode-scalar column for {}",
            anchor.name
        );
        assert_eq!(
            line_prefix.encode_utf16().count() as u32,
            anchor.character,
            "bad UTF-16 column for {}",
            anchor.name
        );
    }

    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", SOURCE).unwrap();
    fixture.write_file("other.R", OTHER_SOURCE).unwrap();
    fixture.write_file("disk.R", DISK_SOURCE).unwrap();
    let main_uri = file_uri(&fixture.path("main.R")).unwrap();
    let other_uri = file_uri(&fixture.path("other.R")).unwrap();
    let disk_uri = file_uri(&fixture.path("disk.R")).unwrap();
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
    let mut session = LspSession::new(client_reader, client_writer);
    let initialize = session.initialize(fixture.root()).await.unwrap();
    assert_eq!(
        initialize.pointer("/capabilities/positionEncoding"),
        Some(&json!("utf-16"))
    );
    let open_mark = session.publication_mark();
    session.open(&main_uri, 1, SOURCE).await.unwrap();
    session.open(&other_uri, 1, OTHER_SOURCE).await.unwrap();

    // Byte -> UTF-16: diagnostics publish the exact range after BMP,
    // combining, astral, and CRLF prefixes.
    let publish = session
        .published_diagnostics_after(&main_uri, open_mark)
        .await
        .unwrap();
    let diagnostic = publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "RY090")
        .expect("length's partial argument name should emit RY090");
    assert_position(&diagnostic["range"]["start"], "diagnostic name");
    assert_position(&diagnostic["range"]["end"], "diagnostic end");

    // Unopened indexed files must retain their source text too; otherwise
    // byte columns leak into LSP ranges.
    let disk_publish = session
        .published_diagnostics_after(&disk_uri, open_mark)
        .await
        .unwrap();
    let disk_diagnostic = disk_publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "RY090")
        .expect("indexed file should publish RY090");
    assert_eq!(
        disk_diagnostic["range"],
        json!({
            "start": {"line": 0, "character": 23},
            "end": {"line": 0, "character": 30}
        })
    );

    // Completion consumes the UTF-16 cursor following an astral scalar.
    let completion = session
        .request(
            "textDocument/completion",
            json!({
                "textDocument": {"uri": main_uri},
                "position": position("completion cursor"),
                "context": {"triggerKind": 2, "triggerCharacter": "$"}
            }),
        )
        .await
        .unwrap();
    assert!(
        completion
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "column")
    );

    let column_completion = completion
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == "column")
        .unwrap();
    // CompletionItem currently relies on the client's default insertion at
    // the consumed cursor; it emits no position-bearing edit of its own.
    assert!(column_completion.get("textEdit").is_none());
    assert!(column_completion.get("additionalTextEdits").is_none());

    let surrogate_interior_completion = session
        .request(
            "textDocument/completion",
            json!({
                "textDocument": {"uri": main_uri},
                "position": {"line": 4, "character": 12},
                "context": {"triggerKind": 2, "triggerCharacter": "$"}
            }),
        )
        .await
        .unwrap();
    assert_eq!(surrogate_interior_completion, Value::Null);

    // Signature help consumes a cursor following BMP and astral scalars and
    // selects the second parameter at the hand-declared UTF-16 column.
    let signature = session
        .request(
            "textDocument/signatureHelp",
            json!({
                "textDocument": {"uri": main_uri}, "position": position("signature cursor")
            }),
        )
        .await
        .unwrap();
    assert_eq!(signature["signatures"][0]["label"], "round(x, digits, ...)");
    assert_eq!(signature["activeParameter"], 1);
    assert!(signature.get("range").is_none());
    let surrogate_interior_signature = session
        .request(
            "textDocument/signatureHelp",
            json!({
                "textDocument": {"uri": main_uri},
                "position": {"line": 5, "character": 13}
            }),
        )
        .await
        .unwrap();
    assert_eq!(surrogate_interior_signature, Value::Null);

    // The state-observable probe through a kept capability: the line-0
    // binding places a type hint right after its identifier. The
    // change-path tests below reuse this probe to observe what the
    // server's cached parse contains after each edit.
    let initial_hints = hints_for_line(&mut session, &main_uri, 0).await;
    assert_hint_at(&initial_hints, 0, 5);

    // The document-change path must reject the same invalid position rather
    // than corrupting the document by snapping into the backtick identifier.
    let invalid_change_mark = session.publication_mark();
    session
        .change(
            &main_uri,
            2,
            json!([{
                "range": {
                    "start": {"line": 8, "character": 2},
                    "end": {"line": 8, "character": 3}
                },
                "text": "BROKEN"
            }]),
        )
        .await
        .unwrap();
    session
        .published_diagnostics_after(&main_uri, invalid_change_mark)
        .await
        .unwrap();
    // The rejected edit must leave the last-good parse serving content
    // requests unchanged.
    let hints_after_invalid_change = hints_for_line(&mut session, &main_uri, 0).await;
    assert_hint_at(&hints_after_invalid_change, 0, 5);

    let out_of_range_change_mark = session.publication_mark();
    session
        .change(
            &main_uri,
            3,
            json!([{
                "range": {
                    "start": {"line": 8, "character": 99},
                    "end": {"line": 8, "character": 99}
                },
                "text": "BROKEN"
            }]),
        )
        .await
        .unwrap();
    session
        .published_diagnostics_after(&main_uri, out_of_range_change_mark)
        .await
        .unwrap();
    let hints_after_out_of_range_change = hints_for_line(&mut session, &main_uri, 0).await;
    assert_hint_at(&hints_after_out_of_range_change, 0, 5);

    // A valid incremental edit after BMP, decomposed combining, and astral
    // scalars must use UTF-16 columns in both endpoints.
    let valid_change_mark = session.publication_mark();
    session
        .change(
            &main_uri,
            4,
            json!([{
                "range": {
                    "start": {"line": 2, "character": 21},
                    "end": {"line": 2, "character": 27}
                },
                "text": "changed"
            }]),
        )
        .await
        .unwrap();
    let changed_publish = session
        .published_diagnostics_after(&main_uri, valid_change_mark)
        .await
        .unwrap();
    assert!(
        changed_publish["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "RY090")
    );
    let completion_after_change = session
        .request(
            "textDocument/completion",
            json!({
                "textDocument": {"uri": main_uri},
                "position": {"line": 4, "character": 22},
                "context": {"triggerKind": 2, "triggerCharacter": "$"}
            }),
        )
        .await
        .unwrap();
    assert!(
        completion_after_change
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "column")
    );

    // The re-parse is visible through the kept surface: the edited
    // line-2 binding is now `changed`, so its hint moves to the new
    // identifier's UTF-16 end column.
    let hints_after_valid_change = hints_for_line(&mut session, &main_uri, 2).await;
    assert_hint_at(&hints_after_valid_change, 2, 28);

    session.shutdown().await.unwrap();
    drop(session);
    tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

// ---------------------------------------------------------------------------
// P35-W10 — Shrinkable LSP session model
//
// A proptest property that generates sequences of LSP operations (initialize,
// open, full/incremental Unicode edits, save, close, restart) and verifies
// that after every quiescent step the live session's published diagnostics
// equal those of a fresh server initialized on the same disk/open-document
// state.
//
// Quiescence is the `textDocument/publishDiagnostics` notification — an
// explicit protocol signal, never a sleep. Before each checkpoint a
// request/response round-trip (inlayHint) acts as a synchronization barrier:
// it drains publications left over from the previous step's multi-URI
// broadcast into the session's pending queue so the subsequent
// `publication_mark` captures only future arrivals.  This is the pattern
// documented on `LspSession::publication_mark`.
//
// The gated alphabet covers only behavior specified in Plans 33–35.  Plan 36
// extends it with workspace-folder mutation, configuration reload, file
// creation/deletion, controlled parse races, and discovery caps.
// ---------------------------------------------------------------------------

use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

type ClientSession = LspSession<
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
>;

/// Three workspace files exercised by the model.
const W10_FILES: &[&str] = &["a.R", "b.R", "c.R"];

/// Initial on-disk content for every workspace file.
const W10_DISK: &str = "x <- 1L\ny <- 2L\n";

/// Source variants with varying Unicode prefixes so incremental-edit ranges
/// exercise BMP, combining-mark, and astral-plane UTF-16 positions.  Each
/// diagnostic variant emits RY090 (partial argument name) and RY091 so the
/// oracle comparison is meaningful.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SourceVariant {
    Clean,
    AsciiDiagnostic,
    BmpDiagnostic,
    AstralDiagnostic,
}

impl SourceVariant {
    fn text(self) -> &'static str {
        match self {
            Self::Clean => "x <- 1L\ny <- 2L\n",
            Self::AsciiDiagnostic => "z <- length(xx = 1L)\ny <- 2L\n",
            Self::BmpDiagnostic => "café <- length(xx = 1L)\ny <- 2L\n",
            Self::AstralDiagnostic => "😀 <- length(xx = 1L)\ny <- 2L\n",
        }
    }

    /// The content of the first line without the trailing newline.  Used as
    /// the replacement text for an incremental range edit targeting line 0.
    fn first_line(self) -> &'static str {
        self.text().lines().next().unwrap()
    }
}

fn source_strategy() -> impl Strategy<Value = SourceVariant> {
    prop_oneof![
        Just(SourceVariant::Clean),
        Just(SourceVariant::AsciiDiagnostic),
        Just(SourceVariant::BmpDiagnostic),
        Just(SourceVariant::AstralDiagnostic),
    ]
}

/// The gated operation alphabet for W10.
#[derive(Clone, Debug)]
enum Operation {
    Open { file: u8, source: SourceVariant },
    FullEdit { file: u8, source: SourceVariant },
    IncrementalEdit { file: u8, source: SourceVariant },
    Save { file: u8 },
    Close { file: u8 },
    Restart,
}

fn operation_strategy() -> impl Strategy<Value = Operation> {
    let file = 0u8..W10_FILES.len() as u8;
    prop_oneof![
        4 => (file.clone(), source_strategy())
            .prop_map(|(file, source)| Operation::Open { file, source }),
        3 => (file.clone(), source_strategy())
            .prop_map(|(file, source)| Operation::FullEdit { file, source }),
        3 => (file.clone(), source_strategy())
            .prop_map(|(file, source)| Operation::IncrementalEdit { file, source }),
        1 => file.clone().prop_map(|file| Operation::Save { file }),
        2 => file.prop_map(|file| Operation::Close { file }),
        1 => Just(Operation::Restart),
    ]
}

fn operation_sequence_strategy() -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(operation_strategy(), 1..10)
}

/// Track which documents are open and their current text.  This mirrors the
/// server's authoritative buffer state and lets the oracle reconstruct the
/// same disk/open-document configuration on a fresh server.
#[derive(Default)]
struct SessionModel {
    open_docs: BTreeMap<u8, String>,
    version: i32,
}

impl SessionModel {
    fn is_open(&self, file: u8) -> bool {
        self.open_docs.contains_key(&file)
    }

    /// Snapshot the open documents as (file_index, text) pairs for the
    /// oracle.
    fn open_docs_snapshot(&self) -> Vec<(u8, String)> {
        self.open_docs
            .iter()
            .map(|(file, text)| (*file, text.clone()))
            .collect()
    }
}

/// Compute the UTF-16 code-unit length of the first line of `text`.  This is
/// the LSP character column at the end of line 0.
fn first_line_utf16_len(text: &str) -> u32 {
    let end = text.find('\n').unwrap_or(text.len());
    text[..end].encode_utf16().count() as u32
}

/// Splice the first-line replacement, matching the server's incremental
/// range-to-byte conversion: replace everything from (0, 0) to
/// (0, first_line_utf16_len) with the new first line, keeping the rest.
fn apply_incremental_edit(old: &str, source: SourceVariant) -> String {
    let first_line_end_byte = old.find('\n').unwrap_or(old.len());
    let mut result = String::with_capacity(old.len() + source.first_line().len());
    result.push_str(source.first_line());
    result.push_str(&old[first_line_end_byte..]);
    result
}

/// Extract and sort the diagnostics array from a publishDiagnostics
/// notification so comparison is order-independent.
fn normalize_diagnostics(publish: &Value) -> Vec<Value> {
    let mut diags = publish
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    diags.sort_by(|a, b| {
        serde_json::to_string(a)
            .unwrap_or_default()
            .cmp(&serde_json::to_string(b).unwrap_or_default())
    });
    diags
}

/// Spawn a fresh server on a duplex pair and initialize the client session.
async fn spawn_session(root: &Path) -> (ClientSession, tokio::task::JoinHandle<()>) {
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move {
        let _ = ry_lsp::run_with(server_reader, server_writer).await;
    });
    let mut session = LspSession::new(client_reader, client_writer);
    session.initialize(root).await.unwrap();
    (session, server)
}

/// Shut down a session and bounded-join its server.
async fn join_session(mut session: ClientSession, server: tokio::task::JoinHandle<()>) {
    let _ = session.shutdown().await;
    drop(session);
    let _ = tokio::time::timeout(Duration::from_secs(3), server).await;
}

/// Start a fresh server on the same fixture root, open the supplied
/// documents, and return the diagnostics published for `target_file`.  This
/// is the oracle: the live session must converge to this result.
async fn fresh_server_diagnostics(
    root: &Path,
    open_docs: &[(u8, String)],
    uris: &[String],
    target_file: u8,
) -> Value {
    let (mut session, server) = spawn_session(root).await;
    // Sync barrier to drain stale publications from the initialize cycle.
    let _ = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": &uris[target_file as usize]},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
            }),
        )
        .await;
    let mark = session.publication_mark();
    for (file, text) in open_docs {
        session.open(&uris[*file as usize], 1, text).await.unwrap();
    }
    let target_uri = &uris[target_file as usize];
    let publish = session
        .published_diagnostics_after(target_uri, mark)
        .await
        .unwrap();
    join_session(session, server).await;
    publish
}

/// Issue a request/response round-trip that drains leftover publications
/// from the previous step's multi-URI broadcast into the session's pending
/// queue.  After this barrier the next `publication_mark` captures only
/// future arrivals — the pattern documented on
/// `LspSession::publication_mark`.
async fn sync_barrier(live: &mut ClientSession, uri: &str) {
    let _ = live
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
            }),
        )
        .await;
}

/// Sync, set a fresh mark, quiesce on the debounced diagnostic publication
/// for the target document, and assert equality with the oracle.
async fn quiesce_and_compare(
    live: &mut ClientSession,
    model: &SessionModel,
    fixture_root: &Path,
    uris: &[String],
    target_file: u8,
    step: usize,
    operation: &Operation,
) -> Result<(), TestCaseError> {
    let target_uri = &uris[target_file as usize];
    sync_barrier(live, target_uri).await;
    let mark = live.publication_mark();
    let live_publish = live
        .published_diagnostics_after(target_uri, mark)
        .await
        .unwrap();
    let fresh_publish =
        fresh_server_diagnostics(fixture_root, &model.open_docs_snapshot(), uris, target_file)
            .await;
    let live_diags = normalize_diagnostics(&live_publish);
    let fresh_diags = normalize_diagnostics(&fresh_publish);
    prop_assert_eq!(
        live_diags,
        fresh_diags,
        "diagnostic mismatch after step {} ({:?}) for {}",
        step,
        operation,
        target_uri
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    /// After every quiescent step, the live session's published diagnostics
    /// for the affected document equal a fresh server initialized on the
    /// same disk/open-document state.  Quiescence is the
    /// `textDocument/publishDiagnostics` notification — never a sleep.
    #[test]
    fn w10_session_converges_to_fresh_server(
        operations in operation_sequence_strategy(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(w10_convergence_property(operations))?;
    }
}

/// Core property logic, factored out of the `proptest!` body so
/// `prop_assert_eq!` (which uses `return`) exits this async block and the
/// error propagates through `block_on`.
async fn w10_convergence_property(operations: Vec<Operation>) -> Result<(), TestCaseError> {
    let fixture = FixtureProject::empty().unwrap();
    for name in W10_FILES {
        fixture.write_file(*name, W10_DISK).unwrap();
    }
    let uris: Vec<String> = W10_FILES
        .iter()
        .map(|name| file_uri(&fixture.path(name)).unwrap())
        .collect();

    let (mut live, mut live_server) = spawn_session(fixture.root()).await;
    let mut model = SessionModel::default();

    for (step, operation) in operations.into_iter().enumerate() {
        match &operation {
            Operation::Open { file, source } => {
                if model.is_open(*file) {
                    continue;
                }
                live.open(&uris[*file as usize], model.version, source.text())
                    .await
                    .unwrap();
                model.version += 1;
                model.open_docs.insert(*file, source.text().to_string());
                quiesce_and_compare(
                    &mut live,
                    &model,
                    fixture.root(),
                    &uris,
                    *file,
                    step,
                    &operation,
                )
                .await?;
            }

            Operation::FullEdit { file, source } => {
                if !model.is_open(*file) {
                    continue;
                }
                live.change(
                    &uris[*file as usize],
                    model.version,
                    json!([{ "text": source.text() }]),
                )
                .await
                .unwrap();
                model.version += 1;
                model.open_docs.insert(*file, source.text().to_string());
                quiesce_and_compare(
                    &mut live,
                    &model,
                    fixture.root(),
                    &uris,
                    *file,
                    step,
                    &operation,
                )
                .await?;
            }

            Operation::IncrementalEdit { file, source } => {
                if !model.is_open(*file) {
                    continue;
                }
                let old_text = &model.open_docs[file];
                let range_end = first_line_utf16_len(old_text);
                live.change(
                    &uris[*file as usize],
                    model.version,
                    json!([{
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": range_end}
                        },
                        "text": source.first_line()
                    }]),
                )
                .await
                .unwrap();
                model.version += 1;
                let new_text = apply_incremental_edit(old_text, *source);
                model.open_docs.insert(*file, new_text.clone());
                quiesce_and_compare(
                    &mut live,
                    &model,
                    fixture.root(),
                    &uris,
                    *file,
                    step,
                    &operation,
                )
                .await?;
            }

            Operation::Save { file } => {
                if !model.is_open(*file) {
                    continue;
                }
                // The server has no didSave handler, so this is a
                // protocol-level no-op.  The sync barrier in the next
                // step's quiesce drains any leftover publications.
                live.notify(
                    "textDocument/didSave",
                    json!({"textDocument": {"uri": &uris[*file as usize]}}),
                )
                .await
                .unwrap();
            }

            Operation::Close { file } => {
                if !model.is_open(*file) {
                    continue;
                }
                let closed_uri = uris[*file as usize].clone();
                // Consume the close's clearing publication here instead of
                // relying on a later step's barrier (#81).  `did_close`
                // publishes `[]` from the notification lane while requests
                // are answered on a concurrent lane, so the clearing
                // notification can be written after a subsequent barrier
                // response; a read-order mark taken after that response
                // then matches the stale `[]` instead of the reopened
                // document's real publication.  The barrier drains
                // in-flight publications for the URI before the mark, and
                // the loop absorbs any populated one written so late that
                // it still sequences after it — only the close's `[]` can
                // terminate the wait.
                sync_barrier(&mut live, &closed_uri).await;
                let clear_mark = live.publication_mark();
                model.open_docs.remove(file);
                live.notify(
                    "textDocument/didClose",
                    json!({"textDocument": {"uri": closed_uri}}),
                )
                .await
                .unwrap();
                loop {
                    let publish = live
                        .published_diagnostics_after(&closed_uri, clear_mark)
                        .await
                        .unwrap();
                    if normalize_diagnostics(&publish).is_empty() {
                        break;
                    }
                }
                // After close, compare the first remaining open document
                // (if any) against the oracle.
                if let Some((&first_open, _)) = model.open_docs.iter().next() {
                    quiesce_and_compare(
                        &mut live,
                        &model,
                        fixture.root(),
                        &uris,
                        first_open,
                        step,
                        &operation,
                    )
                    .await?;
                }
            }

            Operation::Restart => {
                join_session(live, live_server).await;
                let (new_live, new_server) = spawn_session(fixture.root()).await;
                live = new_live;
                live_server = new_server;

                // Re-open all documents that were open before the restart,
                // using the shared version counter to preserve monotonicity.
                for (file, text) in &model.open_docs {
                    live.open(&uris[*file as usize], model.version, text)
                        .await
                        .unwrap();
                    model.version += 1;
                }
                if let Some((&first_open, _)) = model.open_docs.iter().next() {
                    quiesce_and_compare(
                        &mut live,
                        &model,
                        fixture.root(),
                        &uris,
                        first_open,
                        step,
                        &operation,
                    )
                    .await?;
                }
            }
        }
    }

    join_session(live, live_server).await;
    Ok(())
}

/// Regression for #81: after a close/reopen the server must publish the
/// reopened document's diagnostics, and the close's clearing `[]` must not
/// be mistakable for that publication.  Deterministic companion to the
/// convergence property, which samples this path randomly.
#[test]
fn close_reopen_publishes_diagnostics_again() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(close_reopen_republishes());
}

async fn close_reopen_republishes() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", W10_DISK).unwrap();
    let uri = file_uri(&fixture.path("a.R")).unwrap();
    let (mut session, server) = spawn_session(fixture.root()).await;

    // Barrier + mark before the open, the same pattern
    // `fresh_server_diagnostics` uses to drain the initialize cycle's
    // background-index publications.
    sync_barrier(&mut session, &uri).await;
    let open_mark = session.publication_mark();
    session
        .open(&uri, 1, SourceVariant::AsciiDiagnostic.text())
        .await
        .unwrap();
    let first = session
        .published_diagnostics_after(&uri, open_mark)
        .await
        .unwrap();
    assert!(
        !normalize_diagnostics(&first).is_empty(),
        "initial open must publish diagnostics"
    );

    let close_mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
        .await
        .unwrap();
    let cleared = session
        .published_diagnostics_after(&uri, close_mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&cleared).is_empty(),
        "close must clear diagnostics"
    );

    let reopen_mark = session.publication_mark();
    session
        .open(&uri, 2, SourceVariant::AstralDiagnostic.text())
        .await
        .unwrap();
    let republish = session
        .published_diagnostics_after(&uri, reopen_mark)
        .await
        .unwrap();
    assert!(
        !normalize_diagnostics(&republish).is_empty(),
        "reopen must publish diagnostics again, not the close's clearing []"
    );

    join_session(session, server).await;
}
