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

    // Byte -> UTF-16: diagnostics and their structured fix both publish the
    // exact range after BMP, combining, astral, and CRLF prefixes.
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
    let fix = diagnostic
        .pointer("/data/fix")
        .expect("RY090 structured fix");
    assert_position(&fix["range"]["start"], "diagnostic name");
    assert_position(&fix["range"]["end"], "diagnostic name end");
    assert_eq!(fix["replacement"], "x");

    // Unopened indexed files must retain their source text too; otherwise
    // byte columns leak into LSP and structured fixes disappear.
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
    assert_eq!(
        disk_diagnostic["data"]["fix"],
        json!({
            "range": {
                "start": {"line": 0, "character": 23},
                "end": {"line": 0, "character": 25}
            },
            "replacement": "x"
        })
    );

    // UTF-16 -> byte: hover lands on target despite every Unicode class on
    // the preceding part of the line.
    let hover = session
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri}, "position": position("target declaration")
            }),
        )
        .await
        .unwrap();
    assert!(
        hover
            .pointer("/contents/value")
            .and_then(Value::as_str)
            .is_some_and(|value| value.contains("target"))
    );
    assert!(hover.get("range").is_none());

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

    // Rename consumes one UTF-16 position and emits workspace-edit ranges in
    // both the mixed-Unicode CRLF document and another astral document.
    let rename = session
        .request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": main_uri},
                "position": position("target declaration"),
                "newName": "renamed"
            }),
        )
        .await
        .unwrap();
    let main_edits = rename["changes"][&main_uri].as_array().unwrap();
    assert_eq!(main_edits.len(), 2);
    assert_position(&main_edits[0]["range"]["start"], "target declaration");
    assert_position(&main_edits[0]["range"]["end"], "target declaration end");
    assert_position(&main_edits[1]["range"]["start"], "target read");
    assert_position(&main_edits[1]["range"]["end"], "target read end");
    assert_eq!(
        rename["changes"][&other_uri][0]["range"],
        json!({
            "start": {"line": 0, "character": 6},
            "end": {"line": 0, "character": 12}
        })
    );

    // A cursor inside the surrogate pair is not a legal LSP position. The
    // valid boundary resolves the backtick identifier; the interior must not
    // silently snap forward and resolve the same identifier.
    let valid_astral_hover = session
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri}, "position": position("multiline backtick read")
            }),
        )
        .await
        .unwrap();
    assert_ne!(valid_astral_hover, Value::Null);
    let surrogate_interior_hover = session
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri}, "position": {"line": 8, "character": 2}
            }),
        )
        .await
        .unwrap();
    assert_eq!(surrogate_interior_hover, Value::Null);

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
    let hover_after_invalid_change = session
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri}, "position": position("multiline backtick read")
            }),
        )
        .await
        .unwrap();
    assert_ne!(hover_after_invalid_change, Value::Null);

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
    let hover_after_out_of_range_change = session
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri}, "position": position("multiline backtick read")
            }),
        )
        .await
        .unwrap();
    assert_ne!(hover_after_out_of_range_change, Value::Null);

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

    let hover_after_valid_change = session
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": main_uri},
                "position": {"line": 2, "character": 21}
            }),
        )
        .await
        .unwrap();
    assert!(
        hover_after_valid_change
            .pointer("/contents/value")
            .and_then(Value::as_str)
            .is_some_and(|value| value.contains("changed"))
    );

    session.shutdown().await.unwrap();
    drop(session);
    tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
