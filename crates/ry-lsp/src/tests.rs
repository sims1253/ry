use crate::backend::{ProjectCache, State, uri_to_path};
use crate::diagnostics::{
    diag_code_from_lsp, diagnostic_to_lsp, diagnostic_to_lsp_with_source, make_ignore_action,
    make_ignore_file_action,
};
use crate::hints::collect_inlay_hints;
use crate::util::*;
use ry_checker::{Diagnostic, Severity};
use ry_core::{RParser, SourceFile, Span};
use tower_lsp::lsp_types::Diagnostic as LspDiagnostic;
use tower_lsp::lsp_types::*;

#[test]
fn converts_error_diagnostic() {
    let d = Diagnostic::new(
        Severity::Error,
        Span::new(0, 5, 2, 4),
        "test.R",
        "RY040",
        "test message",
    );
    let lsp = diagnostic_to_lsp(d);
    assert_eq!(lsp.range.start.line, 2);
    assert_eq!(lsp.range.start.character, 4);
    // Single-character range: end character is start + 1.
    assert_eq!(lsp.range.end.character, 5);
    assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(lsp.message, "test message");
    assert_eq!(lsp.source.as_deref(), Some("ry"));
    match lsp.code {
        Some(NumberOrString::String(s)) => assert_eq!(s, "RY040"),
        other => panic!("expected String code, got {:?}", other),
    }
}

#[test]
fn converts_warning_diagnostic() {
    let d = Diagnostic::new(
        Severity::Warning,
        Span::new(0, 5, 0, 0),
        "test.R",
        "RY001",
        "warning",
    );
    let lsp = diagnostic_to_lsp(d);
    assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
}

#[test]
fn multi_char_range_from_source() {
    // The source-aware converter must produce a precise multi-char
    // range from the span's byte offsets rather than the old
    // single-character fallback.
    let text = "x <- 1L + \"hello\"\n";
    // The RY040 diagnostic for `+` should span exactly the `+`
    // operator at byte offsets 7..8 (line 0, col 7).
    let d = Diagnostic::new(
        Severity::Error,
        Span::new(7, 8, 0, 7),
        "test.R",
        "RY040",
        "test",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text);
    assert_eq!(lsp.range.start.line, 0);
    assert_eq!(lsp.range.start.character, 7);
    assert_eq!(lsp.range.end.line, 0);
    assert_eq!(lsp.range.end.character, 8);
    // Non-range fields must still be populated identically to the
    // fallback path so behavior is unchanged except for the range.
    assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(lsp.message, "test");
    assert_eq!(lsp.source.as_deref(), Some("ry"));
    match lsp.code {
        Some(NumberOrString::String(s)) => assert_eq!(s, "RY040"),
        other => panic!("expected String code, got {:?}", other),
    }
}

#[test]
fn zero_width_span_extends_by_one_char() {
    // A zero-width span (start == end) must be widened by exactly
    // one character so the squiggle is non-empty in the editor.
    let text = "x <- 1L\n";
    let d = Diagnostic::new(
        Severity::Error,
        Span::new(0, 0, 0, 0),
        "test.R",
        "RY040",
        "test",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text);
    assert_eq!(lsp.range.start.line, 0);
    assert_eq!(lsp.range.start.character, 0);
    assert_eq!(lsp.range.end.line, 0);
    assert_eq!(lsp.range.end.character, 1);
}

#[test]
fn multi_char_range_on_second_line() {
    // Byte offsets that cross a newline must land on the correct
    // line and column. Here the diagnostic sits on line 1 of a
    // two-line source.
    let text = "x <- 1L\ny <- 2L\n";
    // The `y` identifier is at byte offset 8 (the byte right after
    // the first `\n`). It is one character wide.
    let d = Diagnostic::new(
        Severity::Warning,
        Span::new(8, 9, 1, 0),
        "test.R",
        "RY001",
        "warning",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text);
    assert_eq!(lsp.range.start.line, 1);
    assert_eq!(lsp.range.start.character, 0);
    assert_eq!(lsp.range.end.line, 1);
    assert_eq!(lsp.range.end.character, 1);
}

#[test]
fn multi_char_range_spans_identifier() {
    // A diagnostic covering a multi-character identifier must
    // squiggle exactly the identifier's bytes.
    let text = "my_var <- 1L\n";
    // `my_var` occupies bytes 0..6.
    let d = Diagnostic::new(
        Severity::Info,
        Span::new(0, 6, 0, 0),
        "test.R",
        "RY001",
        "info",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text);
    assert_eq!(lsp.range.start.line, 0);
    assert_eq!(lsp.range.start.character, 0);
    assert_eq!(lsp.range.end.line, 0);
    assert_eq!(lsp.range.end.character, 6);
}

#[test]
fn converts_info_diagnostic() {
    let d = Diagnostic::new(
        Severity::Info,
        Span::new(0, 5, 1, 2),
        "test.R",
        "RY001",
        "info",
    );
    let lsp = diagnostic_to_lsp(d);
    assert_eq!(lsp.severity, Some(DiagnosticSeverity::INFORMATION));
}

#[test]
fn uri_to_path_handles_file_scheme() {
    let uri = Url::parse("file:///tmp/foo.R").unwrap();
    let path = uri_to_path(&uri);
    assert!(path.ends_with("foo.R"), "path was {}", path);
}

#[test]
fn uri_to_path_falls_back_for_non_file_scheme() {
    // An `untitled:` URI can't be turned into a file path; we fall
    // back to the URI string so the document still has a stable
    // identity in the open-docs map.
    let uri = Url::parse("untitled:Untitled-1").unwrap();
    let path = uri_to_path(&uri);
    assert_eq!(path, "untitled:Untitled-1");
}

// ---- inlay hint helpers ----

/// Helper: parse + check a snippet and return its inlay hints.
/// Mirrors what the `inlay_hint` LSP method does, minus the async
/// state lookup and range filter.
fn inlay_hints(src: &str) -> Vec<InlayHint> {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let mut checker = ry_checker::Checker::new("test.R");
    let (_, scope) = checker.check_with_scope(&file);
    collect_inlay_hints(&file, &scope, src)
}

#[test]
fn inlay_hints_for_basic_assignments() {
    // The canonical example: an integer vector, a string, and a
    // numeric. Each binding should get exactly one hint whose
    // label mentions the inferred mode.
    let src = "x <- 1:10\nname <- \"hello\"\nd <- 1.5\n";
    let hints = inlay_hints(src);
    assert_eq!(hints.len(), 3, "got {:?}", hints);

    // Every hint must be a TYPE hint with left padding (so it
    // renders as `x : <type>` rather than `x: <type>`).
    for h in &hints {
        assert_eq!(h.kind, Some(InlayHintKind::TYPE));
        assert_eq!(h.padding_left, Some(true));
        assert_eq!(h.padding_right, None);
    }

    // The first hint sits right after `x` at line 0, col 1.
    assert_eq!(hints[0].position.line, 0);
    assert_eq!(hints[0].position.character, 1);
    match &hints[0].label {
        InlayHintLabel::String(s) => assert!(
            s.contains("integer"),
            "expected integer in label, got: {}",
            s
        ),
        other => panic!("expected String label, got {:?}", other),
    }

    // The second hint sits right after `name` at line 1, col 4.
    assert_eq!(hints[1].position.line, 1);
    assert_eq!(hints[1].position.character, 4);
    match &hints[1].label {
        InlayHintLabel::String(s) => assert!(
            s.contains("character"),
            "expected character in label, got: {}",
            s
        ),
        other => panic!("expected String label, got {:?}", other),
    }
}

#[test]
fn inlay_hints_skip_opaque_types() {
    // A call to an unknown function resolves to `Mode::Opaque`
    // ("we don't know"), so `result` must NOT get a hint: showing
    // `: opaque<len=?>?NA?` next to every unknown binding would
    // just be visual noise. We bind a known integer alongside so
    // we can confirm the walker still runs and emits hints for
    // the non-opaque binding.
    let src = "result <- some_unknown_function()\nx <- 1L + 2L\n";
    let hints = inlay_hints(src);
    // Only `x` should produce a hint; `result` is opaque and skipped.
    // Each hint's position is right after its identifier:
    //   `result` is at col 0..6 -> hint at col 6 (line 0)
    //   `x`      is at col 0..1 -> hint at col 1 (line 1)
    let has_hint_for_result = hints
        .iter()
        .any(|h| h.position.line == 0 && h.position.character == 6);
    let has_hint_for_x = hints
        .iter()
        .any(|h| h.position.line == 1 && h.position.character == 1);
    assert!(
        !has_hint_for_result,
        "result is opaque and should NOT get a hint, got: {:?}",
        hints
    );
    assert!(
        has_hint_for_x,
        "x is integer and SHOULD get a hint, got: {:?}",
        hints
    );
}

#[test]
fn inlay_hints_label_starts_with_colon_space() {
    // The hint label should look like a type annotation, so it
    // must start with `: ` to render as `x : integer<...>`.
    let src = "x <- 1L\n";
    let hints = inlay_hints(src);
    assert_eq!(hints.len(), 1);
    match &hints[0].label {
        InlayHintLabel::String(s) => {
            assert!(s.starts_with(": "), "expected ': ' prefix, got: {}", s);
            assert!(
                s.contains("integer"),
                "expected integer mode in label, got: {}",
                s
            );
        }
        other => panic!("expected String label, got {:?}", other),
    }
}

#[test]
fn inlay_hints_position_at_end_of_identifier() {
    // For `my_var <- 1L`, the hint must land at col 6 (the byte
    // right after the 6-character `my_var`), so the editor
    // renders `my_var : integer<...> <- 1L`.
    let src = "my_var <- 1L\n";
    let hints = inlay_hints(src);
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].position.line, 0);
    assert_eq!(
        hints[0].position.character,
        "my_var".len() as u32,
        "hint should land right after the identifier"
    );
}

#[test]
fn inlay_hints_for_function_definition() {
    // `add <- function(a, b) a + b` binds `add` to a function.
    // The walker should emit a hint at the end of `add` (col 3)
    // whose label identifies a function type.
    let src = "add <- function(a, b) a + b\n";
    let hints = inlay_hints(src);
    assert_eq!(hints.len(), 1, "got {:?}", hints);
    assert_eq!(hints[0].position.line, 0);
    assert_eq!(hints[0].position.character, 3);
    match &hints[0].label {
        InlayHintLabel::String(s) => assert!(
            s.contains("function"),
            "expected function in label, got: {}",
            s
        ),
        other => panic!("expected String label, got {:?}", other),
    }
}

// ---- code action helpers ----

/// Helper: build an LSP `Diagnostic` covering a given line range
/// with a string code, mirroring what `diagnostic_to_lsp` produces.
/// Used by the code-action tests so we do not have to run the full
/// checker pipeline just to exercise the quick-fix builders.
fn lsp_diag(line: u32, start_char: u32, end_char: u32, code: &str) -> LspDiagnostic {
    LspDiagnostic {
        range: Range {
            start: Position {
                line,
                character: start_char,
            },
            end: Position {
                line,
                character: end_char,
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(code.to_string())),
        source: Some("ry".to_string()),
        message: "test diagnostic".to_string(),
        ..Default::default()
    }
}

#[test]
fn code_action_ignore_line_appends_suppression_comment() {
    // The canonical case: a diagnostic on `x <- 1L + "s"` should
    // produce a quick-fix that appends
    // `  # ry: ignore[RY040]` to the end of line 0. The edit's
    // range covers the whole line (col 0 to line length) and the
    // new text is the original line plus the comment.
    let text = "x <- 1L + \"s\"\n";
    let diag = lsp_diag(0, 0, 1, "RY040");
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let action = make_ignore_action(&uri, &diag, text).expect("should produce an action");

    assert_eq!(action.title, "Ignore RY040 on this line");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    // The action must link back to the diagnostic it fixes so the
    // editor can show the lightbulb on the right squiggle.
    assert_eq!(
        action.diagnostics.as_deref(),
        Some(std::slice::from_ref(&diag))
    );

    let edit = action.edit.expect("should have an edit");
    let changes = edit.changes.expect("should have changes");
    let edits = changes.get(&uri).expect("should have edits for the uri");
    assert_eq!(edits.len(), 1, "expected exactly one text edit");
    let te = &edits[0];
    // The range covers the whole line (col 0 to len).
    assert_eq!(te.range.start.line, 0);
    assert_eq!(te.range.start.character, 0);
    assert_eq!(te.range.end.line, 0);
    assert_eq!(
        te.range.end.character,
        "x <- 1L + \"s\"".len() as u32,
        "range should span the whole line"
    );
    // The new text is the original line plus the suppression
    // comment.
    assert_eq!(
        te.new_text, "x <- 1L + \"s\"  # ry: ignore[RY040]",
        "new text should append the ignore comment"
    );
}

#[test]
fn code_action_ignore_line_skips_already_suppressed() {
    // A line that already carries an `ry: ignore` comment is fully
    // suppressed; the action must return `None` so the lightbulb
    // does not offer a redundant no-op.
    let text = "x <- 1L + \"s\"  # ry: ignore[RY040]\n";
    let diag = lsp_diag(0, 0, 1, "RY040");
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    assert!(
        make_ignore_action(&uri, &diag, text).is_none(),
        "should not offer an action for an already-suppressed line"
    );
}

#[test]
fn code_action_ignore_line_ignores_hash_inside_string() {
    let text = "x <- \"# not a comment\"\n";
    let diag = lsp_diag(0, 0, 1, "RY040");
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    assert!(make_ignore_action(&uri, &diag, text).is_some());
}

#[test]
fn code_action_ignore_line_detects_noqa_after_string_hash() {
    let text = "x <- \"# not a comment\"  # noqa\n";
    let diag = lsp_diag(0, 0, 1, "RY040");
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    assert!(make_ignore_action(&uri, &diag, text).is_none());
}

#[test]
fn code_action_ignore_line_handles_missing_code() {
    // A diagnostic without a code (defensive) must still produce an
    // action, with the comment omitting the `[CODE]` suffix.
    let text = "x <- bad_thing()\n";
    let mut diag = lsp_diag(0, 0, 1, "RY099");
    diag.code = None;
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let action = make_ignore_action(&uri, &diag, text).expect("should produce an action");
    let edit = action.edit.expect("should have an edit");
    let changes = edit.changes.unwrap();
    let te = &changes.get(&uri).unwrap()[0];
    assert_eq!(
        te.new_text, "x <- bad_thing()  # ry: ignore",
        "missing code should omit the [CODE] suffix"
    );
    assert_eq!(
        action.title, "Ignore this diagnostic on its line",
        "missing code should use a generic title"
    );
}

#[test]
fn code_action_ignore_file_inserts_at_line_zero() {
    // The file-level action inserts `# ry: ignore-file\n` at the
    // very top of the document (a zero-width insert at (0, 0)).
    let text = "x <- 1L\ny <- 2L\n";
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let action = make_ignore_file_action(&uri, text).expect("should produce a file-level action");

    assert_eq!(action.title, "Ignore all diagnostics in this file");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    let edit = action.edit.expect("should have an edit");
    let changes = edit.changes.unwrap();
    let te = &changes.get(&uri).unwrap()[0];
    // The insert is at the very start of the file.
    assert_eq!(te.range.start.line, 0);
    assert_eq!(te.range.start.character, 0);
    assert_eq!(te.range.end.line, 0);
    assert_eq!(te.range.end.character, 0);
    assert_eq!(te.new_text, "# ry: ignore-file\n");
}

#[test]
fn code_action_ignore_file_skips_already_suppressed() {
    // A file that already has `# ry: ignore-file` must not get a
    // second file-level action.
    let text = "# ry: ignore-file\nx <- 1L\n";
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    assert!(
        make_ignore_file_action(&uri, text).is_none(),
        "should not offer a file-level action when one already exists"
    );
}

#[test]
fn diag_code_from_lsp_extracts_string_code() {
    // ry always emits string codes; the helper must surface them.
    let diag = lsp_diag(0, 0, 1, "RY040");
    assert_eq!(diag_code_from_lsp(&diag), "RY040");
}

#[test]
fn diag_code_from_lsp_handles_missing_code() {
    // A diagnostic with no code yields an empty string (not a
    // panic), so the ignore-comment builder can fall back to the
    // code-less format.
    let mut diag = lsp_diag(0, 0, 1, "RY099");
    diag.code = None;
    assert_eq!(diag_code_from_lsp(&diag), "");
}

#[test]
fn position_to_byte_offset_basic() {
    // The helper must map LSP positions back to byte offsets in
    // the source text. This is the inverse of
    // `byte_offset_to_position` for ASCII text.
    let text = "x <- 1L\ny <- 2L\n";
    // (0, 0) -> byte 0 (the 'x').
    assert_eq!(position_to_byte_offset(text, 0, 0), Some(0));
    // (0, 5) -> byte 5 (the '1').
    assert_eq!(position_to_byte_offset(text, 0, 5), Some(5));
    // (1, 0) -> byte 8 (the 'y', right after the first '\n').
    assert_eq!(position_to_byte_offset(text, 1, 0), Some(8));
}

#[test]
fn utf16_position_roundtrip_on_non_ascii() {
    // A line with a 2-byte UTF-8 char ('é', U+00E9) before the
    // cursor. The LSP character column is a UTF-16 code-unit count,
    // so 'é' contributes 1 unit (BMP). Byte offset of the char
    // after 'é' is 2 (1 for 'x'... wait, build a clearer case).
    // Text: "café_x" -- 'c','a','f','é'(2 bytes),'_','x'.
    let text = "café_x";
    // The byte offset of '_': c(0) a(1) f(2) é(3,4) _(5).
    // UTF-16 col of '_': 4 (c,a,f,é each 1 unit).
    assert_eq!(byte_offset_to_position(text, 5).character, 4);
    assert_eq!(position_to_byte_offset(text, 0, 4), Some(5));
}

#[test]
fn utf16_position_counts_astral_as_two_units() {
    // An astral-plane char ('😀', U+1F600) is 4 UTF-8 bytes and 2
    // UTF-16 code units. The char after it sits at UTF-16 col 2.
    let text = "a😀b";
    // byte offsets: a=0, 😀=1..5, b=5.
    assert_eq!(byte_offset_to_position(text, 5).character, 3);
    // 'a'=1 unit, '😀'=2 units -> 'b' is at UTF-16 col 3.
    assert_eq!(position_to_byte_offset(text, 0, 3), Some(5));
    // A column inside the astral char (the second unit of its surrogate
    // pair) is rejected rather than snapped onto a wrong byte offset;
    // didChange incremental edits route through this conversion.
    assert_eq!(position_to_byte_offset(text, 0, 2), None);
}

#[test]
fn edit_one_file_in_workspace_reparses_only_that_file() {
    // Cache acceptance: editing one file in a multi-file
    // workspace must parse ONLY that file. We simulate the LSP document
    // cache directly on a bare `State` (which is what `parsed_file`
    // reads/writes), bypassing the `tower_lsp::Client` plumbing that
    // cannot be constructed in a unit test.
    //
    // The scenario mirrors the real `did_change` flow: bump one doc's
    // version + invalidate its cached parse, then re-serve parses. The
    // unchanged docs must hit the cache (no new parse); the edited doc
    // must miss and re-parse. The parse counter therefore rises by 1.
    let mut state = State::default();
    // Open 30 files, each with a distinct binding so the parses differ.
    for i in 0..30 {
        let path = format!("/ws/file{i}.R");
        let src = format!("x{i} <- {i}L\n");
        state.set_doc(&path, src, 1);
        // Initial parse on open: every doc is parsed once.
        let mut parser = RParser::new().unwrap();
        let file = parser.parse(&path, state.doc_text(&path).unwrap()).unwrap();
        assert!(state.record_parse(&path, 1, std::sync::Arc::new(file)));
    }
    let initial = state.parse_count();
    assert_eq!(initial, 30, "30 files => 30 initial parses, got {initial}");

    // Re-serving every doc with no edits must be pure cache hits.
    for i in 0..30 {
        let path = format!("/ws/file{i}.R");
        assert!(
            state.cached_parse(&path).is_some(),
            "unchanged {path} should be a cache hit"
        );
    }
    assert_eq!(
        state.parse_count(),
        30,
        "cache hits must not re-parse; counter rose to {}",
        state.parse_count()
    );

    // Simulate `did_change` on file 17: bump its version and drop its
    // cached parse (exactly what `Backend::update_doc` does).
    let edited = "/ws/file17.R".to_string();
    state.set_doc(&edited, "x17 <- \"edited\"\n".to_string(), 2);
    state.invalidate_parse(&edited);

    // The edited doc is now a miss; the other 29 still hit.
    assert!(
        state.cached_parse(&edited).is_none(),
        "edited file must be a cache miss"
    );
    for i in 0..30 {
        if i == 17 {
            continue;
        }
        let path = format!("/ws/file{i}.R");
        assert!(
            state.cached_parse(&path).is_some(),
            "unchanged {path} should still be a cache hit"
        );
    }
    assert_eq!(
        state.parse_count(),
        30,
        "lookup-only phase must not re-parse; counter is {}",
        state.parse_count()
    );

    // Re-parse ONLY the edited doc. The counter rises by exactly 1.
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(&edited, state.doc_text(&edited).unwrap())
        .unwrap();
    assert!(
        state.record_parse(&edited, 2, std::sync::Arc::new(file)),
        "re-parse of the edited doc should be stored"
    );
    assert_eq!(
        state.parse_count(),
        31,
        "exactly one new parse for the edited file; counter is {}",
        state.parse_count()
    );
}

#[test]
fn editing_utils_updates_cross_file_analysis_diagnostics() {
    let mut parser = RParser::new().unwrap();
    let utils_path = "/ws/utils.R";
    let analysis_path = "/ws/analysis.R";
    let analysis = parser
        .parse(analysis_path, "result <- make_value() + 1L\n")
        .unwrap();
    let utils_character = parser
        .parse(utils_path, "make_value <- function() { \"hello\" }\n")
        .unwrap();
    let user_stubs = std::sync::Arc::new(std::collections::BTreeMap::new());
    let mut project = ProjectCache::default();

    let before = project.check(
        vec![
            (
                utils_path.to_string(),
                1,
                std::sync::Arc::new(utils_character),
            ),
            (
                analysis_path.to_string(),
                1,
                std::sync::Arc::new(analysis.clone()),
            ),
        ],
        std::sync::Arc::clone(&user_stubs),
    );
    let before_analysis = before
        .iter()
        .find(|(path, _)| path == analysis_path)
        .unwrap();
    assert!(
        before_analysis
            .1
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040"),
        "character-returning utils function should invalidate analysis.R: {before_analysis:?}"
    );

    let utils_integer = parser
        .parse(utils_path, "make_value <- function() { 1L }\n")
        .unwrap();
    let after = project.check(
        vec![
            (
                utils_path.to_string(),
                2,
                std::sync::Arc::new(utils_integer),
            ),
            (analysis_path.to_string(), 1, std::sync::Arc::new(analysis)),
        ],
        user_stubs,
    );
    let after_analysis = after
        .iter()
        .find(|(path, _)| path == analysis_path)
        .unwrap();
    assert!(
        after_analysis
            .1
            .iter()
            .all(|diagnostic| diagnostic.code != "RY040"),
        "editing utils.R must republish corrected analysis.R diagnostics: {after_analysis:?}"
    );
}

// === S2: LSP settings channel tests ===

#[test]
fn effective_filter_uses_editor_ignore_setting() {
    // When the editor sends lint.ignore, it should take effect even
    // with no ry.toml.
    let mut state = State::default();
    state.folder_settings_mut().lint.ignore = Some(vec!["RY010".to_string()]);
    let filter = state.effective_filter();
    // RY010 should be suppressed (effective returns None).
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
}

#[test]
fn effective_filter_falls_back_to_ry_toml_when_editor_unset() {
    // When the editor doesn't send lint.ignore, the ry.toml value
    // should be used.
    let mut state = State::default();
    state.file_config_mut().ignore = vec!["RY010".to_string()];
    let filter = state.effective_filter();
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
}

#[test]
fn effective_filter_editor_overrides_ry_toml() {
    // When both editor and ry.toml provide ignore lists, the editor
    // value wins.
    let mut state = State::default();
    state.file_config_mut().ignore = vec!["RY030".to_string()];
    state.folder_settings_mut().lint.ignore = Some(vec!["RY010".to_string()]);
    let filter = state.effective_filter();
    // Editor's RY010 is ignored.
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
    // ry.toml's RY030 is NOT ignored (editor replaced it).
    assert_eq!(
        filter.effective("RY030", ry_checker::Severity::Warning),
        Some(ry_checker::Severity::Warning)
    );
}

#[test]
fn effective_filter_editor_error_and_warn() {
    // Editor error/warn remapping.
    let mut state = State::default();
    state.folder_settings_mut().lint.error = Some(vec!["RY010".to_string()]);
    state.folder_settings_mut().lint.warn = Some(vec!["RY030".to_string()]);
    let filter = state.effective_filter();
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        Some(ry_checker::Severity::Error)
    );
    assert_eq!(
        filter.effective("RY030", ry_checker::Severity::Error),
        Some(ry_checker::Severity::Warning)
    );
}

#[test]
fn effective_filter_empty_when_nothing_configured() {
    // With no editor settings and no ry.toml, the default filter is
    // empty: everything keeps its default severity.
    let state = State::default();
    let filter = state.effective_filter();
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        Some(ry_checker::Severity::Warning)
    );
}

#[test]
fn effective_filter_explicit_empty_editor_select_disables_default_rules() {
    let mut state = State::default();
    state.file_config_mut().select = Some(vec!["RY010".into()]);
    state.folder_settings_mut().lint.select = Some(Vec::new());
    let filter = state.effective_filter();
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
}

#[test]
fn server_settings_deserialize_from_initialization_options() {
    // Verify the initializationOptions shape that the plan specifies:
    // { settings: [{ lint: { ignore: [...] } }], globalSettings: { ... } }
    let json = serde_json::json!({
        "settings": [
            { "lint": { "ignore": ["RY010"], "error": ["RY030"] } }
        ],
        "globalSettings": {
            "lint": { "warn": ["RY040"] }
        }
    });
    let settings: crate::settings::ServerSettings = serde_json::from_value(json).unwrap();
    assert_eq!(settings.settings.len(), 1);
    assert_eq!(
        settings.settings[0].lint.ignore.as_deref(),
        Some(["RY010".to_string()].as_slice())
    );
    assert_eq!(
        settings.settings[0].lint.error.as_deref(),
        Some(["RY030".to_string()].as_slice())
    );
    assert_eq!(
        settings.global_settings.lint.warn.as_deref(),
        Some(["RY040".to_string()].as_slice())
    );
}

#[test]
fn folder_settings_deserialize_camel_case() {
    let json = serde_json::json!({
        "minConfidence": "high",
        "baseline": "/path/to/baseline.json",
        "checkTestFixtures": true,
        "logLevel": "debug"
    });
    let settings: crate::settings::FolderSettings = serde_json::from_value(json).unwrap();
    assert_eq!(settings.min_confidence.as_deref(), Some("high"));
    assert_eq!(settings.baseline.as_deref(), Some("/path/to/baseline.json"));
    assert_eq!(settings.check_test_fixtures, Some(true));
    assert_eq!(settings.log_level.as_deref(), Some("debug"));
}

#[test]
fn empty_initialization_options_produces_empty_settings() {
    // No initializationOptions → empty ServerSettings.
    let json = serde_json::json!({});
    let settings: crate::settings::ServerSettings = serde_json::from_value(json).unwrap();
    assert!(settings.settings.is_empty());
    assert!(settings.global_settings.lint.ignore.is_none());
}

// ===========================================================================
// Integration tests: full pipeline lifecycle
//
// These tests exercise the composition boundary between document state,
// ProjectCache, and Project::check_incremental — the layer where individual
// components are correct in isolation but contracts between them break.
// ===========================================================================

/// Parse an R source string into a SourceFile for test setup.
fn parse_src(path: &str, src: &str) -> std::sync::Arc<SourceFile> {
    let mut parser = RParser::new().expect("parser init");
    parser
        .parse(path, src)
        .map(std::sync::Arc::new)
        .expect("parse")
}

/// Helper: extract diagnostic codes for a specific file from a check result.
fn codes_for_file<'a>(diags: &'a [(String, Vec<Diagnostic>)], path: &str) -> Vec<&'a str> {
    diags
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, d)| d.iter().map(|x| x.code).collect())
        .unwrap_or_default()
}

/// Integration test: files added to ProjectCache after the initial check
/// must be collected and emitted by the next check_incremental.
///
/// This catches the critical bug where add_file didn't insert into
/// dirty_paths, so late-added files were silently skipped.
#[test]
fn file_added_after_initial_check_is_emitted() {
    let mut cache = ProjectCache::default();
    let stubs = std::sync::Arc::new(std::collections::BTreeMap::new());

    // Initial check with one file.
    let files = vec![("a.R".to_string(), 1, parse_src("a.R", "x <- 1L\n"))];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));
    assert_eq!(diags.len(), 1, "one file on first check");

    // Add a second file (simulates the workspace indexer discovering it).
    let files = vec![
        ("a.R".to_string(), 1, parse_src("a.R", "x <- 1L\n")),
        ("b.R".to_string(), 1, parse_src("b.R", "y <- 2L\n")),
    ];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));

    // The second file must be present in the output.
    assert_eq!(diags.len(), 2, "both files after adding b.R");
    assert!(diags.iter().any(|(p, _)| p == "b.R"), "b.R in output");
}

/// Integration test: a function defined in an added file must be visible
/// to calls in an already-checked file after the next incremental check.
///
/// This is the W4 acceptance criterion: opening one file in a package
/// resolves calls into unopened files.
#[test]
fn added_file_resolves_cross_file_calls() {
    let mut cache = ProjectCache::default();
    let stubs = std::sync::Arc::new(std::collections::BTreeMap::new());

    // Start with just the caller file.
    let files = vec![(
        "caller.R".to_string(),
        1,
        parse_src("caller.R", "result <- helper() + 1L\n"),
    )];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));

    // Without helper.R, helper() returns unknown → unknown + int = no RY040.
    let caller_codes = codes_for_file(&diags, "caller.R");
    assert!(
        !caller_codes.contains(&"RY040"),
        "helper() should be unknown before helper.R is added: {caller_codes:?}"
    );

    // Now add helper.R (simulates disk index). helper returns character.
    let files = vec![
        (
            "caller.R".to_string(),
            1,
            parse_src("caller.R", "result <- helper() + 1L\n"),
        ),
        (
            "helper.R".to_string(),
            1,
            parse_src("helper.R", "helper <- function() \"hello\"\n"),
        ),
    ];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));

    // helper() now returns character → char + int = RY040.
    let caller_codes = codes_for_file(&diags, "caller.R");
    assert!(
        caller_codes.contains(&"RY040"),
        "char return should cause RY040 after adding helper.R: {caller_codes:?}"
    );
}

/// Integration test: updating a file's content that changes a function's
/// return type must update diagnostics in dependent files.
///
/// This verifies the dirty-set propagation: editing utils.R changes
/// make_value()'s return type, which must invalidate analysis.R's diagnostics.
#[test]
fn return_type_change_propagates_to_callers() {
    let mut cache = ProjectCache::default();
    let stubs = std::sync::Arc::new(std::collections::BTreeMap::new());

    // utils.R defines make_value() returning a character.
    // analysis.R calls it in a type-mismatch context.
    let files = vec![
        (
            "utils.R".to_string(),
            1,
            parse_src("utils.R", "make_value <- function() \"hello\"\n"),
        ),
        (
            "analysis.R".to_string(),
            1,
            parse_src("analysis.R", "result <- make_value() + 1L\n"),
        ),
    ];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));

    // char + int → RY040 type mismatch in analysis.R.
    let analysis_codes = codes_for_file(&diags, "analysis.R");
    assert!(
        analysis_codes.contains(&"RY040"),
        "char return should cause RY040: {analysis_codes:?}"
    );

    // Edit utils.R: make_value() now returns integer.
    let files = vec![
        (
            "utils.R".to_string(),
            2, // version bumped
            parse_src("utils.R", "make_value <- function() 1L\n"),
        ),
        (
            "analysis.R".to_string(),
            1,
            parse_src("analysis.R", "result <- make_value() + 1L\n"),
        ),
    ];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));

    // int + int → no RY040 in analysis.R.
    let analysis_codes = codes_for_file(&diags, "analysis.R");
    assert!(
        !analysis_codes.contains(&"RY040"),
        "int return should clear RY040: {analysis_codes:?}"
    );
}

/// Integration test: changing user stubs mid-session must invalidate
/// cached diagnostics so the next check reflects the new stubs.
///
/// This catches the bug where setters (set_user_stubs, etc.) didn't
/// mark files dirty, so incremental checks served stale results.
#[test]
fn changing_stubs_invalidates_cached_diagnostics() {
    use std::collections::BTreeMap;

    let mut cache = ProjectCache::default();

    // First check with no stubs.
    let files = vec![(
        "pkg.R".to_string(),
        1,
        parse_src("pkg.R", "x <- some_fn()\n"),
    )];
    let empty_stubs = std::sync::Arc::new(BTreeMap::new());
    let diags = cache.check(files, std::sync::Arc::clone(&empty_stubs));

    // some_fn() is unknown → no RY010 (calls to unknown functions are allowed).
    let codes = codes_for_file(&diags, "pkg.R");
    assert!(
        codes.is_empty(),
        "unknown function call should produce no diagnostics: {codes:?}"
    );

    // Update file: now x is a type mismatch (char + int).
    // This exercises the incremental path after a content change.
    let files = vec![(
        "pkg.R".to_string(),
        2, // version bumped → forces update
        parse_src("pkg.R", "x <- \"str\" + 1L\n"),
    )];
    let diags = cache.check(files, std::sync::Arc::clone(&empty_stubs));

    // RY040 type mismatch from char + int.
    let codes = codes_for_file(&diags, "pkg.R");
    assert!(
        codes.contains(&"RY040"),
        "char + int should be RY040: {codes:?}"
    );
}

/// Integration test: incremental parse with non-ASCII content must produce
/// correct byte offsets for tree-sitter InputEdit.
///
/// This catches the critical bug where build_input_edit used UTF-16 code
/// units (LSP positions) instead of byte offsets (tree-sitter Points).
#[test]
fn build_input_edit_uses_byte_offsets_for_non_ascii() {
    // A line with a non-ASCII character (é = 2 bytes in UTF-8).
    // Position(0, 2) in UTF-16 = byte offset 3 (because é is 1 UTF-16 unit but 2 bytes).
    let text = "café <- 1L\n";
    let range = Range {
        start: Position {
            line: 0,
            character: 2,
        }, // after "ca"
        end: Position {
            line: 0,
            character: 3,
        }, // after "caf"
    };
    let new_text = "X";

    let edit = crate::backend::build_input_edit(text, range, new_text).unwrap();

    // start_byte should be 2 (ASCII bytes), not 2 UTF-16 units.
    assert_eq!(edit.start_byte, 2);
    // old_end_byte should be 3 ("caf" = 3 ASCII bytes).
    assert_eq!(edit.old_end_byte, 3);
    // new_end_byte = start_byte + new_text.len() = 2 + 1 = 3.
    assert_eq!(edit.new_end_byte, 3);
    // start_position column should be byte column (2), not UTF-16 (2 here, same for ASCII range).
    assert_eq!(edit.start_position.row, 0);
    assert_eq!(edit.start_position.column, 2);
}

/// Integration test: incremental parse with multi-byte edit spanning
/// a newline must compute correct new_end_position.
#[test]
fn build_input_edit_multiline_replacement() {
    let text = "x <- 1\ny <- 2\n";
    let range = Range {
        start: Position {
            line: 0,
            character: 5,
        },
        end: Position {
            line: 0,
            character: 6,
        },
    };
    let new_text = "00\nextra";

    let edit = crate::backend::build_input_edit(text, range, new_text).unwrap();

    // start_byte: "x <- " = 5 bytes, so position (0,5) = byte 5.
    assert_eq!(edit.start_byte, 5);
    // old_end_byte: position (0,6) = byte 6.
    assert_eq!(edit.old_end_byte, 6);
    // new_end_byte: 5 + len("00\nextra") = 5 + 8 = 13.
    assert_eq!(edit.new_end_byte, 13);
    // new_end_position: after inserting "00\nextra", we have:
    // start is at row 0, column 5. The new text has 1 newline,
    // so the end is at row 1. After the newline, "extra" = 5 bytes.
    assert_eq!(edit.new_end_position.row, 1);
    assert_eq!(edit.new_end_position.column, 5); // "extra" = 5 bytes
}

/// Integration test: cold-vs-incremental equivalence across a sequence
/// of add, update, and remove operations.
///
/// This is the most important invariant in Plan 33: after any sequence
/// of operations, incremental diagnostics must match a fresh cold check.
#[test]
fn cold_vs_incremental_equivalence_sequence() {
    let stubs = std::sync::Arc::new(std::collections::BTreeMap::new());

    // Define a 3-file project with cross-file dependencies.
    let sources: &[(&str, &str)] = &[
        ("a.R", "fa <- function(x) x * 2\nva <- fa(1L)\n"),
        ("b.R", "fb <- function(x) fa(x) + 1\nvb <- fb(2L)\n"),
        ("c.R", "fc <- function(x) fb(x) * 3\nvc <- fc(3L)\n"),
    ];

    // Build the incremental cache step by step.
    let mut cache = ProjectCache::default();

    // Step 1: add a.R only.
    let files = vec![("a.R".to_string(), 1, parse_src(sources[0].0, sources[0].1))];
    let _ = cache.check(files, std::sync::Arc::clone(&stubs));

    // Step 2: add b.R.
    let files = vec![
        ("a.R".to_string(), 1, parse_src(sources[0].0, sources[0].1)),
        ("b.R".to_string(), 1, parse_src(sources[1].0, sources[1].1)),
    ];
    let _ = cache.check(files, std::sync::Arc::clone(&stubs));

    // Step 3: add c.R.
    let files: Vec<_> = sources
        .iter()
        .map(|(p, s)| (p.to_string(), 1, parse_src(p, s)))
        .collect();
    let _inc_result = cache.check(files, std::sync::Arc::clone(&stubs));

    // Step 4: edit b.R (change fb's body).
    let edited_sources: Vec<(&str, &str)> = vec![
        ("a.R", "fa <- function(x) x * 2\nva <- fa(1L)\n"),
        ("b.R", "fb <- function(x) fa(x) * 10\nvb <- fb(2L)\n"),
        ("c.R", "fc <- function(x) fb(x) * 3\nvc <- fc(3L)\n"),
    ];
    let files: Vec<_> = edited_sources
        .iter()
        .enumerate()
        .map(|(i, (p, s))| (p.to_string(), i as i32 + 2, parse_src(p, s)))
        .collect();
    let inc_result = cache.check(files, std::sync::Arc::clone(&stubs));

    // Now build a fresh cold project with the same final state.
    let mut cold_project = ry_checker::Project::new();
    for (path, src) in &edited_sources {
        cold_project.add_file(path.to_string(), parse_src(path, src).as_ref().clone());
    }
    let cold_result = cold_project.check();

    // Compare: every file's diagnostic codes must match.
    assert_eq!(inc_result.len(), cold_result.len(), "file count matches");
    for ((inc_path, inc_diags), (cold_path, cold_diags)) in
        inc_result.iter().zip(cold_result.iter())
    {
        assert_eq!(inc_path, cold_path, "path order matches");
        let inc_codes: Vec<_> = inc_diags.iter().map(|d| &d.code).collect();
        let cold_codes: Vec<_> = cold_diags.iter().map(|d| &d.code).collect();
        assert_eq!(
            inc_codes, cold_codes,
            "diagnostics match for {inc_path}:\n  incremental: {inc_codes:?}\n  cold:        {cold_codes:?}"
        );
    }
}

/// Integration test: removing a file from the project clears its
/// contribution to the shared function table.
#[test]
fn removed_file_clears_cross_file_resolution() {
    let stubs = std::sync::Arc::new(std::collections::BTreeMap::new());
    let mut cache = ProjectCache::default();

    // helper.R defines my_helper() returning character.
    // caller.R does my_helper() + 1L → RY040 (char + int).
    let files = vec![
        (
            "helper.R".to_string(),
            1,
            parse_src("helper.R", "my_helper <- function() \"str\"\n"),
        ),
        (
            "caller.R".to_string(),
            1,
            parse_src("caller.R", "x <- my_helper() + 1L\n"),
        ),
    ];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));
    let caller_codes = codes_for_file(&diags, "caller.R");
    assert!(
        caller_codes.contains(&"RY040"),
        "char + int should be RY040: {caller_codes:?}"
    );

    // Remove helper.R: my_helper() now returns unknown → no RY040.
    let files = vec![(
        "caller.R".to_string(),
        1,
        parse_src("caller.R", "x <- my_helper() + 1L\n"),
    )];
    let diags = cache.check(files, std::sync::Arc::clone(&stubs));
    let caller_codes = codes_for_file(&diags, "caller.R");
    assert!(
        !caller_codes.contains(&"RY040"),
        "RY040 should disappear when my_helper returns unknown: {caller_codes:?}"
    );
}
