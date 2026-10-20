//! Open documents, keyed by URI and grouped by display path.
//!
//! The checker and the disk index key files by the lossy display path from
//! [`uri_to_path`], so two non-UTF-8 filenames can share one key. Each
//! document keeps its own text, version, tree, parse, and hints, and the
//! most recently opened or edited document at a display path is the one
//! that path analyzes.

use super::{path_to_uri, uri_to_path};
use crate::positions::{byte_offset_to_point, position_to_byte_offset};
use ry_core::SourceFile;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::{InlayHint, Range, TextDocumentContentChangeEvent, Url};

pub(super) struct CachedHints {
    pub stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    pub hints: Vec<InlayHint>,
}

/// One open buffer. `tree`, `parsed`, and `hints` describe the current
/// text and are dropped by every edit.
pub(super) struct Document {
    pub uri: Url,
    pub native: Option<PathBuf>,
    pub text: String,
    pub version: i32,
    /// Unique across all documents and bumped by every edit, so a parse
    /// started before an edit, a close, or a reopen cannot be installed.
    pub edit: u64,
    pub tree: Option<ry_core::Tree>,
    pub parsed: Option<Arc<SourceFile>>,
    pub hints: Option<CachedHints>,
}

#[derive(Default)]
pub(super) struct Documents {
    /// Display path to its documents, least recently edited first.
    by_path: HashMap<String, Vec<Document>>,
    edits: u64,
}

impl Documents {
    pub(super) fn open(&mut self, uri: Url, text: String, version: i32) {
        self.edits += 1;
        let docs = self.by_path.entry(uri_to_path(&uri)).or_default();
        docs.retain(|doc| doc.uri != uri);
        docs.push(Document {
            native: uri.to_file_path().ok(),
            uri,
            text,
            version,
            edit: self.edits,
            tree: None,
            parsed: None,
            hints: None,
        });
    }

    /// Apply one `didChange` batch. Returns whether any change applied.
    pub(super) fn change(
        &mut self,
        uri: &Url,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: i32,
    ) -> bool {
        let path = uri_to_path(uri);
        let Some((docs, index)) = self.by_path.get_mut(&path).and_then(|docs| {
            let index = docs.iter().position(|doc| &doc.uri == uri)?;
            Some((docs, index))
        }) else {
            tracing::warn!(%uri, "ignoring change for a closed URI");
            return false;
        };
        let doc = &mut docs[index];
        // An invalid UTF-16 range stops the batch. Prior valid edits remain
        // committed. Later ranges refer to text we could not produce.
        let mut applied = false;
        for change in changes {
            match change.range {
                Some(range) => {
                    let Some((start, end)) = range_byte_span(&doc.text, range) else {
                        tracing::error!(
                            ?range,
                            "invalid UTF-16 range in document change; aborting remaining changes in didChange batch for {path}; server and client text will desynchronize until a full sync is received"
                        );
                        break;
                    };
                    if let Some(tree) = &mut doc.tree {
                        tree.edit(&input_edit(&doc.text, start, end, &change.text));
                    }
                    doc.text.replace_range(start..end, &change.text);
                }
                None => {
                    doc.text = change.text;
                    doc.tree = None;
                }
            }
            applied = true;
        }
        // An empty or immediately rejected batch changes nothing, including
        // which document the display path analyzes.
        if !applied {
            return false;
        }
        self.edits += 1;
        doc.edit = self.edits;
        doc.version = version;
        doc.parsed = None;
        doc.hints = None;
        let doc = docs.remove(index);
        docs.push(doc);
        true
    }

    pub(super) fn close(&mut self, uri: &Url) {
        let path = uri_to_path(uri);
        if let Some(docs) = self.by_path.get_mut(&path) {
            docs.retain(|doc| &doc.uri != uri);
            if docs.is_empty() {
                self.by_path.remove(&path);
            }
        }
    }

    /// Display paths with at least one open document.
    pub(super) fn paths(&self) -> Vec<String> {
        self.by_path.keys().cloned().collect()
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.by_path.contains_key(path)
    }

    /// Every document open at `path`.
    pub(super) fn at(&self, path: &str) -> &[Document] {
        self.by_path.get(path).map_or(&[], Vec::as_slice)
    }

    /// The document `path` analyzes: the most recently edited one.
    pub(super) fn active(&self, path: &str) -> Option<&Document> {
        self.at(path).last()
    }

    pub(super) fn active_mut(&mut self, path: &str) -> Option<&mut Document> {
        self.by_path.get_mut(path)?.last_mut()
    }

    /// Where diagnostics for `path` go: its open URIs in a stable order, or
    /// the display URI when none is open.
    pub(super) fn uris(&self, path: &str) -> Vec<Url> {
        let mut uris: Vec<Url> = self.at(path).iter().map(|doc| doc.uri.clone()).collect();
        if uris.is_empty() {
            return vec![path_to_uri(path)];
        }
        uris.sort();
        uris
    }
}

/// Convert an LSP range (0-based line/character, UTF-16 code units) in
/// `old_text` to a byte-offset span for splicing.
fn range_byte_span(old_text: &str, range: Range) -> Option<(usize, usize)> {
    let start_byte = position_to_byte_offset(old_text, range.start.line, range.start.character)?;
    let end_byte = position_to_byte_offset(old_text, range.end.line, range.end.character)?;
    (start_byte <= end_byte).then_some((start_byte, end_byte))
}

/// The tree-sitter edit for replacing `old_text[start_byte..old_end_byte]`
/// with `new_text`, so the next parse reuses unchanged subtrees.
fn input_edit(
    old_text: &str,
    start_byte: usize,
    old_end_byte: usize,
    new_text: &str,
) -> ry_core::InputEdit {
    let start_position = byte_offset_to_point(old_text, start_byte);
    let new_end_position = match new_text.rfind('\n') {
        None => ry_core::Point {
            row: start_position.row,
            column: start_position.column + new_text.len(),
        },
        Some(last_newline) => ry_core::Point {
            row: start_position.row + new_text.matches('\n').count(),
            column: new_text.len() - last_newline - 1,
        },
    };
    ry_core::InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte: start_byte + new_text.len(),
        start_position,
        old_end_position: byte_offset_to_point(old_text, old_end_byte),
        new_end_position,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{State, parsed_file};
    use super::*;
    use ry_core::RParser;
    use tower_lsp::lsp_types::Position;

    fn edit(line: u32, start: u32, end: u32, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(line, start),
                end: Position::new(line, end),
            }),
            range_length: None,
            text: text.to_string(),
        }
    }

    #[test]
    fn ranged_edits_keep_the_incremental_tree_and_stop_at_invalid_utf16() {
        let uri = Url::parse("file:///tmp/ordinary.R").unwrap();
        let path = uri_to_path(&uri);
        let original = "x <- 1L\ny <- 2L\n";
        let mut docs = Documents::default();
        docs.open(uri.clone(), original.to_string(), 1);
        let (_, tree) = RParser::new()
            .unwrap()
            .parse_with_tree(&path, original, None)
            .unwrap();
        docs.by_path.get_mut(&path).unwrap()[0].tree = Some(tree);

        // A multi-byte replacement, then one that inserts a line.
        let changes = vec![edit(0, 5, 7, "\"😀\""), edit(1, 5, 7, "3L\nz <- 4L")];
        assert!(docs.change(&uri, changes, 2));
        let expected = "x <- \"😀\"\ny <- 3L\nz <- 4L\n";
        let doc = docs.active(&path).unwrap();
        assert_eq!(doc.text, expected);
        assert_eq!(doc.version, 2);
        let edited_tree = doc.tree.as_ref().expect("ranged edits retain the tree");
        let (_, incremental) = RParser::new()
            .unwrap()
            .parse_with_tree(&path, expected, Some(edited_tree))
            .unwrap();
        let (_, cold) = RParser::new()
            .unwrap()
            .parse_with_tree(&path, expected, None)
            .unwrap();
        assert_eq!(
            incremental.root_node().to_sexp(),
            cold.root_node().to_sexp()
        );

        // UTF-16 column 7 lands inside the astral character. The following
        // full replacement must not run after this malformed ranged edit.
        let invalid = vec![
            edit(0, 7, 7, "bad"),
            TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "stale <- TRUE\n".to_string(),
            },
        ];
        assert!(!docs.change(&uri, invalid, 3));
        let doc = docs.active(&path).unwrap();
        assert_eq!(doc.text, expected);
        assert_eq!(doc.version, 2);
        assert!(doc.tree.is_some());
    }

    #[test]
    fn colliding_uris_keep_their_own_text_and_the_last_edit_is_active() {
        let first = Url::parse("file:///tmp/bad%FF.R").unwrap();
        let second = Url::parse("file:///tmp/bad%FE.R").unwrap();
        let path = uri_to_path(&first);
        assert_eq!(path, uri_to_path(&second));
        let mut docs = Documents::default();
        docs.open(first.clone(), "first🙂\n#| x integer\n".to_string(), 1);
        docs.open(second.clone(), "g\n# ordinary\n".to_string(), 1);
        assert_eq!(docs.active(&path).unwrap().uri, second);

        assert!(docs.change(&first, vec![edit(0, 0, 7, "one")], 7));
        assert_eq!(docs.active(&path).unwrap().uri, first);
        assert!(docs.change(&second, vec![edit(0, 0, 1, "two")], 7));
        assert_eq!(docs.active(&path).unwrap().uri, second);
        let texts: Vec<_> = docs.at(&path).iter().map(|doc| doc.text.as_str()).collect();
        assert_eq!(texts, ["one\n#| x integer\n", "two\n# ordinary\n"]);
        assert_eq!(docs.uris(&path).len(), 2);

        docs.close(&second);
        assert_eq!(docs.active(&path).unwrap().uri, first);
        docs.close(&first);
        assert!(!docs.contains(&path));
        assert_eq!(docs.uris(&path), [path_to_uri(&path)]);
    }

    #[tokio::test]
    async fn an_edit_reparses_only_the_edited_document() {
        let mut state = State::default();
        for i in 0..3 {
            let uri = Url::parse(&format!("file:///ws/file{i}.R")).unwrap();
            state.docs.open(uri, format!("x{i} <- {i}L\n"), 1);
        }
        let state = tokio::sync::Mutex::new(state);
        let mut parses = Vec::new();
        for i in 0..3 {
            let (file, _) = parsed_file(&state, &format!("/ws/file{i}.R"))
                .await
                .unwrap();
            parses.push(file);
        }

        let edited = Url::parse("file:///ws/file1.R").unwrap();
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "x1 <- \"edited\"\n".to_string(),
        };
        assert!(state.lock().await.docs.change(&edited, vec![change], 2));
        for (i, before) in parses.iter().enumerate() {
            let (after, text) = parsed_file(&state, &format!("/ws/file{i}.R"))
                .await
                .unwrap();
            assert_eq!(Arc::ptr_eq(before, &after), i != 1, "file{i}.R");
            assert_eq!(after.source, text);
        }
    }
}
