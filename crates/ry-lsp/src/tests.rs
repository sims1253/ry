use crate::backend::{FolderAnalysisContext, ProjectCache, uri_to_path};
use crate::diagnostics::{
    diag_code_from_lsp, diagnostic_to_lsp_with_source, make_ignore_action, make_ignore_file_action,
};
use crate::hints::collect_inlay_hints;
use crate::positions::*;
use crate::settings::LintSettings;
use ry_checker::{Diagnostic, Severity};
use ry_core::{RParser, SourceFile, Span};
use tower_lsp::lsp_types::Diagnostic as LspDiagnostic;
use tower_lsp::lsp_types::*;

#[test]
fn multi_char_range_from_source() {
    let text = "x <- 1L + \"hello\"\n";
    // The `+` sits at bytes 7..8.
    let d = Diagnostic::new(
        Severity::Error,
        Span::new(7, 8, 0, 7),
        "test.R",
        "RY040",
        "test",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text, None);
    assert_eq!(lsp.range.start.line, 0);
    assert_eq!(lsp.range.start.character, 7);
    assert_eq!(lsp.range.end.line, 0);
    assert_eq!(lsp.range.end.character, 8);
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
    let text = "x <- 1L\n";
    let d = Diagnostic::new(
        Severity::Error,
        Span::new(0, 0, 0, 0),
        "test.R",
        "RY040",
        "test",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text, None);
    assert_eq!(lsp.range.start.line, 0);
    assert_eq!(lsp.range.start.character, 0);
    assert_eq!(lsp.range.end.line, 0);
    assert_eq!(lsp.range.end.character, 1);
}

#[test]
fn multi_char_range_on_second_line() {
    let text = "x <- 1L\ny <- 2L\n";
    // `y` is at byte 8, right after the first newline.
    let d = Diagnostic::new(
        Severity::Warning,
        Span::new(8, 9, 1, 0),
        "test.R",
        "RY001",
        "warning",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text, None);
    assert_eq!(lsp.range.start.line, 1);
    assert_eq!(lsp.range.start.character, 0);
    assert_eq!(lsp.range.end.line, 1);
    assert_eq!(lsp.range.end.character, 1);
}

#[test]
fn multi_char_range_spans_identifier() {
    let text = "my_var <- 1L\n";
    // `my_var` occupies bytes 0..6.
    let d = Diagnostic::new(
        Severity::Info,
        Span::new(0, 6, 0, 0),
        "test.R",
        "RY001",
        "info",
    );
    let lsp = diagnostic_to_lsp_with_source(&d, text, None);
    assert_eq!(lsp.range.start.line, 0);
    assert_eq!(lsp.range.start.character, 0);
    assert_eq!(lsp.range.end.line, 0);
    assert_eq!(lsp.range.end.character, 6);
}

#[test]
fn uri_to_path_handles_file_scheme() {
    let uri = Url::parse("file:///tmp/foo.R").unwrap();
    let path = uri_to_path(&uri);
    assert!(path.ends_with("foo.R"), "path was {}", path);
}

#[test]
fn uri_to_path_falls_back_for_non_file_scheme() {
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
    checker.enable_assignment_capture();
    checker.check(&file);
    collect_inlay_hints(&checker.take_assignment_types(), src)
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
fn inlay_hints_name_known_environment_and_language_values() {
    let hints = inlay_hints("e <- new.env()\nq <- quote(a + b)\nu <- unknown_call()\n");
    let labels: Vec<_> = hints
        .iter()
        .map(|hint| match &hint.label {
            InlayHintLabel::String(label) => label.as_str(),
            other => panic!("expected String label, got {other:?}"),
        })
        .collect();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(labels[0].contains("environment"), "{labels:?}");
    assert!(labels[1].contains("language"), "{labels:?}");
}

#[test]
fn inlay_hint_summary_and_detail_keep_schema_meanings_separate() {
    let hints = inlay_hints("x <- list(a = 1L, b = 2L, c = 3L, d = 4L)\n");
    assert_eq!(hints.len(), 1);
    let InlayHintLabel::String(label) = &hints[0].label else {
        panic!("expected string label");
    };
    assert!(label.contains("+1 known fields; complete"), "{label}");
    let Some(InlayHintTooltip::String(detail)) = &hints[0].tooltip else {
        panic!("expected expanded detail");
    };
    assert!(detail.contains("\"d\": integer"), "{detail}");
    assert!(detail.contains("locally constructed: true"), "{detail}");
}

#[test]
fn inlay_hint_expands_classed_union_from_function_body() {
    let hints = inlay_hints(
        "f <- function(flag) {\n  x <- structure(if (flag) 1L else \"a\", class = \"tagged\")\n  x\n}\n",
    );
    let hint = hints
        .iter()
        .find(
            |hint| matches!(&hint.label, InlayHintLabel::String(label) if label.contains("union")),
        )
        .expect("assignment of classed union has a hint");
    let Some(InlayHintTooltip::String(detail)) = &hint.tooltip else {
        panic!("expected expanded detail");
    };
    assert!(detail.contains("class=[\"tagged\"]"), "{detail}");
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

#[test]
fn inlay_hints_keep_each_assignments_type() {
    let hints = inlay_hints("x <- 1L\nx <- \"later\"\ny <- x\nx <- FALSE\n");
    assert_eq!(hints.len(), 4, "{hints:?}");
    for (hint, mode) in hints
        .iter()
        .zip(["integer", "character", "character", "logical"])
    {
        assert!(
            matches!(&hint.label, InlayHintLabel::String(label) if label.contains(mode)),
            "{hint:?}"
        );
    }
}

#[test]
fn inlay_hints_use_the_assignments_lexical_scope() {
    let hints = inlay_hints("x <- 1L\nf <- function() {\n  x <- \"local\"\n  x\n}\n");
    assert_eq!(hints.len(), 3, "{hints:?}");
    assert_eq!(hints[2].position, Position::new(2, 3));
    assert!(
        matches!(&hints[2].label, InlayHintLabel::String(label) if label.contains("character"))
    );
}

#[test]
fn inlay_hints_skip_quoted_assignments() {
    let hints = inlay_hints("base::quote(x <- 1L)\nz <- TRUE\n");
    assert_eq!(hints.len(), 1, "{hints:?}");
    assert_eq!(hints[0].position, Position::new(1, 1));
}

#[test]
fn inlay_hints_do_not_use_later_mutations() {
    let hints = inlay_hints("x <- 1L\nclass(x) <- \"special\"\nnames(x) <- \"name\"\n");
    assert_eq!(hints.len(), 1, "{hints:?}");
    assert!(
        matches!(&hints[0].label, InlayHintLabel::String(label) if label == ": integer<len=1>")
    );
}

#[test]
fn inlay_hints_for_nested_assignments_and_unicode() {
    let hints = inlay_hints("变量 <- other <- 1L\n");
    assert_eq!(hints.len(), 2, "{hints:?}");
    assert_eq!(hints[0].position, Position::new(0, 2));
    assert_eq!(hints[1].position, Position::new(0, 11));
}

#[test]
fn inlay_hints_follow_backtick_token_ends() {
    let hints = inlay_hints("`a b` <- 1L\n`multi\nline` <- FALSE\n");
    assert_eq!(hints.len(), 2, "{hints:?}");
    assert_eq!(hints[0].position, Position::new(0, 5));
    assert_eq!(hints[1].position, Position::new(2, 5));
}

// ---- code action helpers ----

/// An LSP diagnostic on one line range, for the quick-fix builders.
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
    // The edit replaces line 0 with itself plus `  # ry: ignore[RY040]`.
    let text = "x <- 1L + \"s\"\n";
    let diag = lsp_diag(0, 0, 1, "RY040");
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let action = make_ignore_action(&uri, &diag, &parse_src("test.R", text))
        .expect("should produce an action");

    assert_eq!(action.title, "Ignore RY040 on this line");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(
        action.diagnostics.as_deref(),
        Some(std::slice::from_ref(&diag))
    );

    let edit = action.edit.expect("should have an edit");
    let changes = edit.changes.expect("should have changes");
    let edits = changes.get(&uri).expect("should have edits for the uri");
    assert_eq!(edits.len(), 1, "expected exactly one text edit");
    let te = &edits[0];
    assert_eq!(te.range.start.line, 0);
    assert_eq!(te.range.start.character, 0);
    assert_eq!(te.range.end.line, 0);
    assert_eq!(
        te.range.end.character,
        "x <- 1L + \"s\"".len() as u32,
        "range should span the whole line"
    );
    assert_eq!(
        te.new_text, "x <- 1L + \"s\"  # ry: ignore[RY040]",
        "new text should append the ignore comment"
    );
}

/// The line quick-fix is offered exactly when it would change what the
/// checker's own suppression parser suppresses: a directive that already
/// covers the code (trailing, or standalone on a preceding comment line)
/// withholds it, and so does an edit that would leave a still-malformed
/// directive or target a comment-only line. A code-less diagnostic is
/// fixed with a bare ignore.
#[test]
fn code_action_ignore_line_availability_follows_checker_suppressions() {
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let cases: &[(&str, u32, Option<&str>, bool)] = &[
        (
            "x <- 1L + \"s\"  # ry: ignore[RY040]\n",
            0,
            Some("RY040"),
            false,
        ),
        (
            "x <- 1L + \"s\"  # ry: ignore[RY010]\n",
            0,
            Some("RY040"),
            true,
        ),
        (
            "x <- 1L + \"s\"  # ry: ignore[RY010]\n",
            0,
            Some("RY010"),
            false,
        ),
        ("x <- 1L + \"s\"  # ry: ignore\n", 0, Some("RY040"), false),
        (
            "x <- 1L + \"s\"  # RY: IGNORE[RY040]\n",
            0,
            Some("RY040"),
            false,
        ),
        (
            "x <- 1L + \"s\"  # ry: ignore-file\n",
            0,
            Some("RY040"),
            true,
        ),
        (
            "x <- 1L + \"s\"  # see docs for ry: ignore\n",
            0,
            Some("RY040"),
            true,
        ),
        ("x <- \"# not a comment\"\n", 0, Some("RY040"), true),
        (
            "x <- \"# not a comment\"  # noqa\n",
            0,
            Some("RY040"),
            false,
        ),
        ("# ry: ignore\n\"a\" + 1L\n", 0, Some("RY040"), false),
        (
            "# ry: ignore[RY010]\nz <- length(xx = 1L)\n",
            1,
            Some("RY010"),
            false,
        ),
        (
            "# ry: ignore[RY040]\nz <- length(xx = 1L)\n",
            1,
            Some("RY010"),
            true,
        ),
        (
            "# ry: ignore\nz <- length(xx = 1L)\n",
            1,
            Some("RY040"),
            false,
        ),
        (
            "# ry: ignore[RY010]\n# why this is fine\n\nz <- length(xx = 1L)\n",
            3,
            Some("RY010"),
            false,
        ),
        (
            "# ry: ignore[RY010]\ny <- 1L\nz <- length(xx = 1L)\n",
            2,
            Some("RY010"),
            true,
        ),
        ("\"a\" + 1L  # ry: ignore[RX040]\n", 0, Some("RY040"), true),
        (
            "\"a\" + 1L  # ry: ignore[RY999999]\n",
            0,
            Some("RY040"),
            true,
        ),
        ("\"a\" + 1L  # noqa: E501\n", 0, Some("RY040"), true),
        ("\"a\" + 1L  # noqa-ish note\n", 0, Some("RY040"), true),
        ("\"a\" + 1L  # noqa: E501, RY040\n", 0, Some("RY040"), false),
        (
            "\"a\" + 1L  # ry: ignore[RY040]]\n",
            0,
            Some("RY040"),
            false,
        ),
        (
            "\"a\" + 1L  # ry: ignore[RY040] ]\n",
            0,
            Some("RY040"),
            false,
        ),
        (
            "\"a\" + 1L  # ry: ignore[RY040][RX040]\n",
            0,
            Some("RY040"),
            false,
        ),
        ("\"a\" + 1L  # noqa[RY040]]\n", 0, Some("RY040"), false),
        ("\"a\" + 1L # ry: ignore[RY040]]\n", 0, None, false),
        ("\"a\" + 1L # ry: ignore[RY040] ]\n", 0, None, false),
        ("\"a\" + 1L # noqa[RY040]]\n", 0, None, false),
        ("\"a\" + 1L # ry: ignore[RY010] reason\n", 0, None, true),
    ];
    for &(text, line, code, offered) in cases {
        let mut diag = lsp_diag(line, 0, 1, code.unwrap_or("RY040"));
        if code.is_none() {
            diag.code = None;
        }
        let action = make_ignore_action(&uri, &diag, &parse_src("test.R", text));
        assert_eq!(action.is_some(), offered, "{code:?} on {text:?}");
    }
}

#[test]
fn code_action_ignore_line_handles_missing_code() {
    let text = "x <- bad_thing()\n";
    let mut diag = lsp_diag(0, 0, 1, "RY099");
    diag.code = None;
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let action = make_ignore_action(&uri, &diag, &parse_src("test.R", text))
        .expect("should produce an action");
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
    let text = "x <- 1L\ny <- 2L\n";
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let action = make_ignore_file_action(&uri, &parse_src("test.R", text))
        .expect("should produce a file-level action");

    assert_eq!(action.title, "Ignore all diagnostics in this file");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    let edit = action.edit.expect("should have an edit");
    let changes = edit.changes.unwrap();
    let te = &changes.get(&uri).unwrap()[0];
    assert_eq!(te.range.start.line, 0);
    assert_eq!(te.range.start.character, 0);
    assert_eq!(te.range.end.line, 0);
    assert_eq!(te.range.end.character, 0);
    assert_eq!(te.new_text, "# ry: ignore-file\n");
}

#[test]
fn file_suppression_preserves_the_shebang() {
    let uri = Url::parse("file:///tmp/script.R").unwrap();
    for source in [
        "#!/usr/bin/env Rscript\nx <- missing\n",
        "#!/usr/bin/env Rscript",
    ] {
        let action = make_ignore_file_action(&uri, &parse_src("script.R", source)).unwrap();
        let edit = &action.edit.unwrap().changes.unwrap()[&uri][0];
        assert_eq!(edit.range.start, edit.range.end);
        let offset =
            position_to_byte_offset(source, edit.range.start.line, edit.range.start.character)
                .unwrap();
        let mut updated = source.to_string();
        updated.insert_str(offset, &edit.new_text);
        assert!(updated.starts_with("#!/usr/bin/env Rscript\n# ry: ignore-file\n"));
    }
}

#[test]
fn code_action_ignore_file_skips_already_suppressed() {
    let text = "# ry: ignore-file\nx <- 1L\n";
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    assert!(
        make_ignore_file_action(&uri, &parse_src("test.R", text)).is_none(),
        "should not offer a file-level action when one already exists"
    );
}

#[test]
fn line_suppression_never_inserts_inside_multiline_tokens() {
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    for source in [
        "x <- \"first\nlast\"\n",
        "identity(`first\nlast` = 1)\n",
        "x <- r\"(first\nlast)\"\n",
    ] {
        let file = parse_src("test.R", source);
        assert!(make_ignore_action(&uri, &lsp_diag(0, 0, 1, "RY040"), &file).is_none());
    }
}

#[test]
fn code_action_preserves_multiline_string_context() {
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let file = parse_src(
        "test.R",
        "x <- \"first\n# ry: ignore[RY010]\nlast\"\ny <- missing\n",
    );
    let diag = lsp_diag(2, 0, 1, "RY010");
    assert!(make_ignore_action(&uri, &diag, &file).is_some());
}

#[test]
fn diag_code_from_lsp_extracts_string_code() {
    let diag = lsp_diag(0, 0, 1, "RY040");
    assert_eq!(diag_code_from_lsp(&diag), "RY040");
}

#[test]
fn diag_code_from_lsp_handles_missing_code() {
    let mut diag = lsp_diag(0, 0, 1, "RY099");
    diag.code = None;
    assert_eq!(diag_code_from_lsp(&diag), "");
}

#[test]
fn position_to_byte_offset_basic() {
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
    // The LSP character column is a UTF-16 code-unit count, so 'é'
    // contributes 1 unit despite being 2 UTF-8 bytes.
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
    assert_eq!(position_to_byte_offset(text, 0, 1), Some(1));
    // A column inside the astral char (the second unit of its surrogate
    // pair) is rejected rather than snapped onto a wrong byte offset;
    // didChange incremental edits route through this conversion.
    assert_eq!(position_to_byte_offset(text, 0, 2), None);
}

#[test]
fn utf16_columns_skip_crlf_carriage_return() {
    // The `\r` of a CRLF terminator is not a column character: the
    // position at the `\n` (or at end-of-line) is the column before
    // the `\r`, and line 1 starts at column 0.
    let text = "ab\r\ncd";
    // Byte offsets: a=0 b=1 \r=2 \n=3 c=4 d=5.
    assert_eq!(byte_offset_to_position(text, 2), Position::new(0, 2));
    assert_eq!(byte_offset_to_position(text, 3), Position::new(0, 2));
    assert_eq!(byte_offset_to_position(text, 4), Position::new(1, 0));
    // Inverse: (0, 2) resolves to the `\r` byte, (1, 0) to `c`.
    assert_eq!(position_to_byte_offset(text, 0, 2), Some(2));
    assert_eq!(position_to_byte_offset(text, 1, 0), Some(4));
    // Contrast: a LONE `\r` (no `\n` after it) neither starts a new line
    // nor vanishes from the column count — it is one column wide.
    let lone = "ab\rcd";
    assert_eq!(byte_offset_to_position(lone, 3), Position::new(0, 3));
    assert_eq!(position_to_byte_offset(lone, 0, 3), Some(3));
}

#[test]
fn byte_offset_to_point_counts_byte_columns() {
    // tree-sitter Point columns are BYTES from the line start, unlike
    // LSP's UTF-16 unit — a 2-byte precomposed é advances the column
    // by 2, and a CRLF `\r` is an ordinary column byte.
    let text = "caf\u{e9}\r\nx";
    // Byte offsets: c=0 a=1 f=2 \u{e9}=3,4 \r=5 \n=6 x=7.
    let after_e_acute = byte_offset_to_point(text, 5);
    assert_eq!(after_e_acute.row, 0);
    assert_eq!(after_e_acute.column, 5);
    let on_lf = byte_offset_to_point(text, 6);
    assert_eq!(on_lf.row, 0);
    assert_eq!(on_lf.column, 6);
    let x = byte_offset_to_point(text, 7);
    assert_eq!(x.row, 1);
    assert_eq!(x.column, 0);
    let crlf = byte_offset_to_point("a\r\nb", 3);
    assert_eq!((crlf.row, crlf.column), (1, 0));
    assert_eq!(byte_offset_to_point("aé", 2), byte_offset_to_point("aé", 1));
    // Offsets past the end clamp to the text end.
    let clamped = byte_offset_to_point(text, 999);
    assert_eq!(clamped.row, 1);
    assert_eq!(clamped.column, 1);
}

#[test]
fn line_start_returns_byte_offset_of_line() {
    // `\u{e9}` (precomposed é) is 2 bytes, so the byte layout is
    // a=0 b=1 \n=2 c=3 d=4 \u{e9}=5,6 f=7 \n=8 g=9 h=10. Line starts:
    // line 0 at 0, line 1 after the first \n at 3, line 2 after the
    // second \n at 9.
    let text = "ab\ncd\u{e9}f\ngh";
    assert_eq!(line_start(text, 0), 0);
    assert_eq!(line_start(text, 1), 3);
    assert_eq!(line_start(text, 2), 9);
    // A line index past the end clamps to the end of the text.
    assert_eq!(line_start(text, 7), text.len());
    assert_eq!(line_start("", 0), 0);
}

/// The severity filter a folder compiles from its `ry.toml` and the
/// editor's lint settings.
fn effective_filter(config: ry_config::Config, lint: LintSettings) -> ry_checker::SeverityFilter {
    ry_checker::filter_from_config(&lint.apply(&config))
}

fn rules(codes: &[&str]) -> Option<Vec<String>> {
    Some(codes.iter().map(|code| code.to_string()).collect())
}

#[test]
fn effective_filter_uses_editor_ignore_setting() {
    let lint = LintSettings {
        ignore: rules(&["RY010"]),
        ..Default::default()
    };
    let filter = effective_filter(Default::default(), lint);
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
}

#[test]
fn effective_filter_falls_back_to_ry_toml_when_editor_unset() {
    let config = ry_config::Config {
        ignore: vec!["RY010".to_string()],
        ..Default::default()
    };
    let filter = effective_filter(config, Default::default());
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
}

#[test]
fn effective_filter_editor_overrides_ry_toml() {
    let config = ry_config::Config {
        ignore: vec!["RY030".to_string()],
        ..Default::default()
    };
    let lint = LintSettings {
        ignore: rules(&["RY010"]),
        ..Default::default()
    };
    let filter = effective_filter(config, lint);
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
    // The editor list replaces the `ry.toml` list.
    assert_eq!(
        filter.effective("RY030", ry_checker::Severity::Warning),
        Some(ry_checker::Severity::Warning)
    );
}

#[test]
fn effective_filter_editor_error_and_warn() {
    let lint = LintSettings {
        error: rules(&["RY010"]),
        warn: rules(&["RY030"]),
        ..Default::default()
    };
    let filter = effective_filter(Default::default(), lint);
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
    let filter = effective_filter(Default::default(), Default::default());
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        Some(ry_checker::Severity::Warning)
    );
}

#[test]
fn effective_filter_explicit_empty_editor_select_disables_default_rules() {
    let config = ry_config::Config {
        select: rules(&["RY010"]),
        ..Default::default()
    };
    let lint = LintSettings {
        select: Some(Vec::new()),
        ..Default::default()
    };
    let filter = effective_filter(config, lint);
    assert_eq!(
        filter.effective("RY010", ry_checker::Severity::Warning),
        None
    );
}

#[test]
fn server_settings_deserialize_from_initialization_options() {
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
        "enable": false
    });
    let settings: crate::settings::FolderSettings = serde_json::from_value(json).unwrap();
    assert_eq!(settings.min_confidence, Some(ry_checker::Confidence::High));
    assert_eq!(settings.baseline.as_deref(), Some("/path/to/baseline.json"));
    assert_eq!(settings.enable, Some(false));
}

#[test]
fn empty_initialization_options_produces_empty_settings() {
    let json = serde_json::json!({});
    let settings: crate::settings::ServerSettings = serde_json::from_value(json).unwrap();
    assert!(settings.settings.is_empty());
    assert!(settings.global_settings.lint.ignore.is_none());
}

fn parse_src(path: &str, src: &str) -> std::sync::Arc<SourceFile> {
    let mut parser = RParser::new().expect("parser init");
    parser
        .parse(path, src)
        .map(std::sync::Arc::new)
        .expect("parse")
}

fn check(
    cache: &mut ProjectCache,
    files: Vec<(String, i32, std::sync::Arc<SourceFile>)>,
    stubs: std::sync::Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
) -> Vec<(String, Vec<Diagnostic>)> {
    cache
        .check_with_workspace(
            files,
            stubs,
            Default::default(),
            Vec::new(),
            &Default::default(),
        )
        .diagnostics
}

/// Diagnostic codes for one file of a check result.
fn codes_for_file<'a>(diags: &'a [(String, Vec<Diagnostic>)], path: &str) -> Vec<&'a str> {
    diags
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, d)| d.iter().map(|x| x.code).collect())
        .unwrap_or_default()
}

/// A content change re-checks a cached file under the same stubs.
#[test]
fn changing_stubs_invalidates_cached_diagnostics() {
    use std::collections::BTreeMap;

    let mut cache = ProjectCache::default();

    let files = vec![(
        "pkg.R".to_string(),
        1,
        parse_src("pkg.R", "x <- some_fn()\n"),
    )];
    let empty_stubs = std::sync::Arc::new(BTreeMap::new());
    let diags = check(&mut cache, files, std::sync::Arc::clone(&empty_stubs));

    // Calls to unknown functions are allowed.
    let codes = codes_for_file(&diags, "pkg.R");
    assert!(
        codes.is_empty(),
        "unknown function call should produce no diagnostics: {codes:?}"
    );

    let files = vec![(
        "pkg.R".to_string(),
        2,
        parse_src("pkg.R", "x <- \"str\" + 1L\n"),
    )];
    let diags = check(&mut cache, files, std::sync::Arc::clone(&empty_stubs));

    let codes = codes_for_file(&diags, "pkg.R");
    assert!(
        codes.contains(&"RY040"),
        "char + int should be RY040: {codes:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Stub reload classification: "no directories configured" makes the empty
// map the new state; "every configured directory failed" retains the
// previous stubs.
// ──────────────────────────────────────────────────────────────────────────

/// A one-function stub pinning the named package.
fn package_stub(package: &str) -> String {
    serde_json::json!({
        "schema_version": "1",
        "package": package,
        "version": "test",
        "functions": {
            "value": {"params": [], "return": {"mode": "integer", "length": "1"}}
        }
    })
    .to_string()
}

/// A folder context whose `ry.toml` is `config`, with two stub
/// directories on disk: `stubs/` (package `localdep`) and `other/`
/// (package `otherdep`).
fn stub_reload_context(config: &str) -> (ry_testkit::FixtureProject, FolderAnalysisContext) {
    let fixture = ry_testkit::FixtureProject::empty().unwrap();
    fixture.write_file("ry.toml", config).unwrap();
    fixture
        .write_file("stubs/localdep.json", package_stub("localdep"))
        .unwrap();
    fixture
        .write_file("other/otherdep.json", package_stub("otherdep"))
        .unwrap();
    let ctx = FolderAnalysisContext::load(fixture.root().to_path_buf(), Default::default(), None);
    (fixture, ctx)
}

fn rebuild_folder_context(old: &FolderAnalysisContext) -> FolderAnalysisContext {
    FolderAnalysisContext::load(old.root.clone(), old.folder_settings.clone(), Some(old))
}

#[test]
fn rebuild_clears_stubs_when_the_last_typeshed_directory_is_removed() {
    let (fixture, old) = stub_reload_context("typeshed = [\"stubs\"]\n");
    assert!(old.stubs.contains_key("localdep"));
    // Clearing the list and deleting the setting both declare no
    // directories: the empty reload is the intended new state, not a
    // failure, so the previous map must be replaced (warm converges with
    // a fresh server on the same config).
    for (name, config) in [
        ("empty list", Some("typeshed = []\n")),
        ("deleted setting", Some("")),
        ("deleted file", None),
    ] {
        if let Some(config) = config {
            fixture.write_file("ry.toml", config).unwrap();
        } else {
            std::fs::remove_file(fixture.path("ry.toml")).unwrap();
        }
        let rebuilt = rebuild_folder_context(&old);
        assert!(
            rebuilt.stubs.is_empty(),
            "{name}: removing the last typeshed directory must clear the stub map; got {:?}",
            rebuilt.stubs.keys().collect::<Vec<_>>()
        );
        assert!(
            rebuilt.config.typeshed.is_empty(),
            "{name}: the reloaded config declares no typeshed directories"
        );
    }
}

#[test]
fn rebuild_retains_stubs_when_every_configured_directory_fails() {
    let (fixture, old) = stub_reload_context("typeshed = [\"stubs\"]\n");
    assert!(old.stubs.contains_key("localdep"));
    // A configured-but-unreadable directory (here: nonexistent) is a
    // genuine reload failure; the previous stubs survive it.
    fixture
        .write_file("ry.toml", "typeshed = [\"missing\"]\n")
        .unwrap();
    let rebuilt = rebuild_folder_context(&old);
    assert!(
        rebuilt.stubs.contains_key("localdep"),
        "a failed stub directory reload must retain the previous stub map; got {:?}",
        rebuilt.stubs.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        rebuilt.config.typeshed,
        vec![fixture.path("missing")],
        "the failed config itself is the new config; only the stubs are retained"
    );
}

#[test]
fn rebuild_replaces_stubs_when_switching_to_a_different_directory() {
    let (fixture, old) = stub_reload_context("typeshed = [\"stubs\"]\n");
    assert!(old.stubs.contains_key("localdep"));
    fixture
        .write_file("ry.toml", "typeshed = [\"other\"]\n")
        .unwrap();
    let rebuilt = rebuild_folder_context(&old);
    let packages: Vec<_> = rebuilt.stubs.keys().collect();
    assert_eq!(
        packages,
        vec!["otherdep"],
        "switching directories must load only the new directory's stubs"
    );
}

#[test]
fn rebuild_takes_partial_stubs_when_one_of_several_directories_fails() {
    // Retention is only for the every-directory-failed case. A partial
    // failure keeps the successes, as a fresh server would.
    let (fixture, old) = stub_reload_context("typeshed = [\"stubs\", \"other\"]\n");
    let packages: Vec<_> = old.stubs.keys().collect();
    assert_eq!(packages, vec!["localdep", "otherdep"]);
    fixture
        .write_file("ry.toml", "typeshed = [\"stubs\", \"missing\"]\n")
        .unwrap();
    let rebuilt = rebuild_folder_context(&old);
    let packages: Vec<_> = rebuilt.stubs.keys().collect();
    assert_eq!(
        packages,
        vec!["localdep"],
        "a partially failed reload must keep only the loadable directory's stubs"
    );
}

#[test]
fn rebuild_clears_stubs_when_the_only_stub_file_is_malformed() {
    let (fixture, old) = stub_reload_context("typeshed = [\"stubs\"]\n");
    assert!(old.stubs.contains_key("localdep"));
    // The directory still loads; only its sole file is malformed. That
    // is a per-file warning, not a directory failure, so the empty map
    // is the new state — the same result a fresh server would produce.
    fixture
        .write_file("stubs/localdep.json", "not json")
        .unwrap();
    let rebuilt = rebuild_folder_context(&old);
    assert!(
        rebuilt.stubs.is_empty(),
        "a malformed stub file must not retain the previous stub map; got {:?}",
        rebuilt.stubs.keys().collect::<Vec<_>>()
    );
}

/// `a.R` and `z.R` both define `f`, and sorted order makes `z.R`'s integer
/// `f` win for `use.R`. Closing and reopening `a.R` must not flip the
/// winner, although `update_file` appends a re-added path at the end.
#[test]
fn close_reopen_does_not_flip_cross_file_shadowing() {
    let stubs = std::sync::Arc::new(std::collections::BTreeMap::new());
    let (a_src, z_src, use_src) = (
        "f <- function() \"str\"\n",
        "f <- function() 1L\n",
        "x <- f() + 1L\n",
    );
    let open_all = || {
        vec![
            ("a.R".to_string(), 1, parse_src("a.R", a_src)),
            ("use.R".to_string(), 1, parse_src("use.R", use_src)),
            ("z.R".to_string(), 1, parse_src("z.R", z_src)),
        ]
    };
    let mut cache = ProjectCache::default();

    let diags = check(&mut cache, open_all(), std::sync::Arc::clone(&stubs));
    assert!(
        !codes_for_file(&diags, "use.R").contains(&"RY040"),
        "z.R's integer f wins in canonical order: {:?}",
        codes_for_file(&diags, "use.R")
    );

    // did_close: the path leaves the cache entirely (no disk entry).
    let files = vec![
        ("use.R".to_string(), 1, parse_src("use.R", use_src)),
        ("z.R".to_string(), 1, parse_src("z.R", z_src)),
    ];
    let _ = check(&mut cache, files, std::sync::Arc::clone(&stubs));

    // did_open of the unchanged file: re-added, content identical.
    let diags = check(&mut cache, open_all(), std::sync::Arc::clone(&stubs));
    assert!(
        !codes_for_file(&diags, "use.R").contains(&"RY040"),
        "close/reopen must not re-append a.R at the end and flip the winner: {:?}",
        codes_for_file(&diags, "use.R")
    );

    // The warm cache must match a cold project built in canonical order.
    let mut cold = ry_checker::Project::new();
    cold.add_file("a.R".to_string(), parse_src("a.R", a_src).as_ref().clone());
    cold.add_file(
        "use.R".to_string(),
        parse_src("use.R", use_src).as_ref().clone(),
    );
    cold.add_file("z.R".to_string(), parse_src("z.R", z_src).as_ref().clone());
    let cold_diags = cold.check();
    for path in ["a.R", "use.R", "z.R"] {
        assert_eq!(
            codes_for_file(&diags, path),
            codes_for_file(&cold_diags, path),
            "warm and cold must agree for {path}"
        );
    }
}

/// A file joining after the first check takes its sorted position, not the
/// end, so `z.R`'s `f` keeps winning when `a.R` joins later.
#[test]
fn late_added_file_takes_canonical_position_not_the_end() {
    let stubs = std::sync::Arc::new(std::collections::BTreeMap::new());
    let (a_src, z_src, use_src) = (
        "f <- function() \"str\"\n",
        "f <- function() 1L\n",
        "x <- f() + 1L\n",
    );
    let mut cache = ProjectCache::default();

    let files = vec![("z.R".to_string(), 1, parse_src("z.R", z_src))];
    let _ = check(&mut cache, files, std::sync::Arc::clone(&stubs));

    let files = vec![
        ("a.R".to_string(), 1, parse_src("a.R", a_src)),
        ("use.R".to_string(), 1, parse_src("use.R", use_src)),
        ("z.R".to_string(), 1, parse_src("z.R", z_src)),
    ];
    let diags = check(&mut cache, files, std::sync::Arc::clone(&stubs));
    assert!(
        !codes_for_file(&diags, "use.R").contains(&"RY040"),
        "late-joining a.R must not shadow z.R by landing at the end: {:?}",
        codes_for_file(&diags, "use.R")
    );
}
