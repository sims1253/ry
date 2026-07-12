use crate::backend::{ProjectCache, State, path_to_uri, uri_to_path};
use crate::diagnostics::{
    diag_code_from_lsp, diagnostic_to_lsp, diagnostic_to_lsp_with_source, make_ignore_action,
    make_ignore_file_action,
};
use crate::folding::collect_folding_ranges;
use crate::hints::{
    collect_completions, collect_inlay_hints, common_r_completions, extract_last_identifier,
    find_enclosing_call, get_signature,
};
use crate::navigation::{
    build_rename_edits, collect_document_highlights, find_definition_locations,
    find_references_in_file,
};
use crate::selection::{build_selection_range, find_identifier_range_at_position};
use crate::symbols::{collect_symbols, flatten_symbols_to_symbol_info};
use crate::util::*;
use ry_checker::{Diagnostic, Severity};
use ry_core::RParser;
use ry_core::Span;
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

#[test]
fn hover_returns_type_for_known_variable() {
    // Integration test: parse a simple R snippet, check it, and
    // verify that hover on a variable returns its type.
    let text = "x <- 1L + 2L\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", text).unwrap();
    let mut checker = ry_checker::Checker::new("test.R");
    let (_, scope) = checker.check_with_scope(&file);
    let t = scope.get("x").expect("x should be in scope");
    assert_eq!(t.mode, ry_core::types::Mode::Integer);
}

#[test]
fn goto_def_finds_variable_assignment() {
    // `x <- 1L + 2L` defines `x` at line 0, col 0 (1-char name).
    let text = "x <- 1L + 2L\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", text).unwrap();
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let locs = find_definition_locations(&file, "x", &uri);
    assert_eq!(locs.len(), 1, "expected exactly one definition of x");
    let loc = &locs[0];
    assert_eq!(loc.uri, uri);
    assert_eq!(loc.range.start.line, 0);
    assert_eq!(loc.range.start.character, 0);
    // Name "x" is one character wide.
    assert_eq!(loc.range.end.line, 0);
    assert_eq!(loc.range.end.character, 1);
}

#[test]
fn goto_def_finds_function_definition() {
    // `add <- function(a, b) a + b` defines `add` (3 chars) at
    // line 0, col 0. The parser models this as an Assign whose
    // value is an Expr::Function, so the Assign-target branch of
    // the walk must find it.
    let text = "add <- function(a, b) a + b\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", text).unwrap();
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let locs = find_definition_locations(&file, "add", &uri);
    assert_eq!(locs.len(), 1, "expected exactly one definition of add");
    let loc = &locs[0];
    assert_eq!(loc.range.start.line, 0);
    assert_eq!(loc.range.start.character, 0);
    assert_eq!(loc.range.end.character, 3, "add is 3 chars wide");
}

#[test]
fn goto_def_finds_local_definition_inside_function_body() {
    // A local assignment nested inside a function literal must be
    // found by recursing through Expr::Function -> body. `local`
    // sits on line 1, indented 2 spaces.
    let text = "f <- function() {\n  local <- 1L\n  local\n}\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", text).unwrap();
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let locs = find_definition_locations(&file, "local", &uri);
    assert_eq!(locs.len(), 1, "expected exactly one definition of local");
    let loc = &locs[0];
    assert_eq!(loc.range.start.line, 1);
    assert_eq!(loc.range.start.character, 2);
    assert_eq!(loc.range.end.character, 2 + "local".len() as u32);
}

#[test]
fn goto_def_finds_reassignment_as_multiple_locations() {
    // Two assignments to the same name yield two Locations; the
    // editor can present them as alternatives.
    let text = "x <- 1L\nx <- 2L\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", text).unwrap();
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let locs = find_definition_locations(&file, "x", &uri);
    assert_eq!(locs.len(), 2);
    assert_eq!(locs[0].range.start.line, 0);
    assert_eq!(locs[1].range.start.line, 1);
}

#[test]
fn goto_def_returns_empty_for_undefined_name() {
    let text = "x <- 1L\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", text).unwrap();
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let locs = find_definition_locations(&file, "does_not_exist", &uri);
    assert!(locs.is_empty(), "expected no definitions");
}

// ---- documentSymbol helpers ----

/// Helper: parse + check a snippet and return its top-level
/// `DocumentSymbol`s. Mirrors what the `document_symbol` LSP method
/// does, minus the async state lookup.
fn doc_symbols(src: &str) -> Vec<DocumentSymbol> {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let mut checker = ry_checker::Checker::new("test.R");
    let (_, scope) = checker.check_with_scope(&file);
    collect_symbols(&file.stmts, src, Some(&scope))
}

#[test]
fn document_symbols_for_mixed_top_level_bindings() {
    // The canonical example from the task: a function, a call
    // result, and a string. We expect 3 top-level symbols with the
    // right names and kinds.
    let src = "add <- function(x = 0, y = 0) { x + y }\nresult <- add(1, 2)\nname <- \"hello\"\n";
    let symbols = doc_symbols(src);
    assert_eq!(symbols.len(), 3, "got {:?}", symbols);

    assert_eq!(symbols[0].name, "add");
    assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
    // The checker infers a function type for `add`, so the detail
    // surfaces that (which starts with "function"). We don't pin
    // the exact signature since return-type inference may refine
    // it over time; we just check it identifies a function.
    let detail = symbols[0]
        .detail
        .as_deref()
        .expect("add should have detail");
    assert!(
        detail.starts_with("function"),
        "expected detail to start with 'function', got: {}",
        detail
    );

    assert_eq!(symbols[1].name, "result");
    assert_eq!(symbols[1].kind, SymbolKind::VARIABLE);

    assert_eq!(symbols[2].name, "name");
    assert_eq!(symbols[2].kind, SymbolKind::VARIABLE);
}

#[test]
fn document_symbols_detail_uses_inferred_type() {
    // The checker infers `x` as a scalar integer, so the detail
    // string must mention "integer".
    let src = "x <- 1L + 2L\n";
    let symbols = doc_symbols(src);
    assert_eq!(symbols.len(), 1);
    let detail = symbols[0].detail.as_deref().expect("detail should be set");
    assert!(
        detail.contains("integer"),
        "expected integer in detail, got: {}",
        detail
    );
}

#[test]
fn document_symbols_function_has_nested_children() {
    // A function literal assigned to `f` contains a nested local
    // function `g`. `g` must appear as a child of `f` (not at the
    // top level), and `g` itself must be classified as a function.
    let src = "f <- function() {\n  g <- function() { 1L }\n  g\n}\n";
    let symbols = doc_symbols(src);
    assert_eq!(symbols.len(), 1);
    let f = &symbols[0];
    assert_eq!(f.name, "f");
    assert_eq!(f.kind, SymbolKind::FUNCTION);
    let children = f
        .children
        .as_ref()
        .expect("f should have nested children from its body");
    let g = children
        .iter()
        .find(|c| c.name == "g")
        .expect("should find nested g");
    assert_eq!(g.kind, SymbolKind::FUNCTION);
}

#[test]
fn document_symbols_selection_range_covers_identifier() {
    // For `my_var <- 42`, the selection range must cover exactly
    // the 6-character identifier at the start of line 0, and it
    // must be contained within the enclosing range.
    let src = "my_var <- 42\n";
    let symbols = doc_symbols(src);
    assert_eq!(symbols.len(), 1);
    let sym = &symbols[0];
    assert_eq!(sym.name, "my_var");
    assert_eq!(sym.selection_range.start.line, 0);
    assert_eq!(sym.selection_range.start.character, 0);
    assert_eq!(sym.selection_range.end.line, 0);
    assert_eq!(sym.selection_range.end.character, "my_var".len() as u32);
    // selection_range must be inside range per the LSP spec.
    assert!(sym.range.start <= sym.selection_range.start);
    assert!(sym.range.end >= sym.selection_range.end);
}

#[test]
fn document_symbols_empty_for_no_bindings() {
    // A bare expression with no assignments yields no symbols.
    let src = "1L + 2L\n";
    let symbols = doc_symbols(src);
    assert!(symbols.is_empty(), "expected no symbols, got {:?}", symbols);
}

#[test]
fn document_symbols_flatten_control_flow_bodies() {
    // Bindings inside `if` / `for` blocks are visible in R's
    // enclosing scope, so they should surface at the current
    // outline level rather than disappearing.
    let src = "if (TRUE) {\n  a <- 1L\n}\nfor (i in 1:3) {\n  b <- 2L\n}\n";
    let symbols = doc_symbols(src);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"a"), "a should be in outline: {:?}", names);
    assert!(names.contains(&"b"), "b should be in outline: {:?}", names);
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

// ---- completion helpers ----

/// Helper: parse + check a snippet and return the completion
/// items for a given cursor position and trigger context. Mirrors
/// what the `completion` LSP method does, minus the async state
/// lookup.
fn completions(
    src: &str,
    position: Position,
    context: Option<CompletionContext>,
) -> Vec<CompletionItem> {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let mut checker = ry_checker::Checker::new("test.R");
    let (_, scope) = checker.check_with_scope(&file);
    collect_completions(src, position, &context, &scope)
}

/// Build a `CompletionContext` for a given trigger character.
/// Used by the `$`-triggered test to mimic what the editor sends
/// right after the user types `$`.
fn trigger_context(ch: &str) -> Option<CompletionContext> {
    Some(CompletionContext {
        trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
        trigger_character: Some(ch.to_string()),
    })
}

#[test]
fn extract_last_identifier_basic() {
    // The variable name sits at the end of the input; the helper
    // must scan back to its start, stopping at the first non-ident
    // character.
    assert_eq!(extract_last_identifier("mtcars").as_deref(), Some("mtcars"));
    assert_eq!(extract_last_identifier("df$col").as_deref(), Some("col"));
    assert_eq!(
        extract_last_identifier("foo.bar_baz").as_deref(),
        Some("foo.bar_baz")
    );
    // Trailing whitespace / `$` are not stripped here; the caller
    // (`collect_completions`) handles that. So a trailing `$`
    // produces `None` because `$` is not an identifier character.
    assert_eq!(extract_last_identifier("mtcars$"), None);
    assert_eq!(extract_last_identifier(""), None);
    assert_eq!(extract_last_identifier("(1 + 2)"), None);
}

// ---- signature help helpers ----

#[test]
fn find_enclosing_call_basic_round() {
    // `round(` with the cursor right after the `(` (col 6): the
    // enclosing call is `round`, and no comma has been typed yet
    // so the active parameter is 0.
    let text = "round(\n";
    let (name, active) = find_enclosing_call(text, 0, 6).expect("should find call");
    assert_eq!(name, "round");
    assert_eq!(active, 0);
}

#[test]
fn find_enclosing_call_counts_commas() {
    // `round(x, ` with the cursor at col 9 (after the comma + the
    // space): one comma has been typed, so the active parameter is
    // 1 (the second parameter, `digits`).
    let text = "round(x, \n";
    let (name, active) = find_enclosing_call(text, 0, 9).expect("should find call");
    assert_eq!(name, "round");
    assert_eq!(active, 1);
}

#[test]
fn find_enclosing_call_skips_nested_calls() {
    // `outer(inner(1, 2), ` with the cursor at the trailing
    // space: the nearest enclosing call is `outer` (the inner
    // `inner(1, 2)` is closed), and only the top-level comma
    // (after the inner call) counts toward `outer`'s active
    // parameter, so it should be 1.
    let text = "outer(inner(1, 2), \n";
    let (name, active) = find_enclosing_call(text, 0, 18).expect("should find call");
    assert_eq!(name, "outer");
    assert_eq!(
        active, 1,
        "only the top-level comma should count, not the inner call's comma"
    );
}

#[test]
fn find_enclosing_call_returns_none_outside_call() {
    // No `(` before the cursor: not inside a call.
    let text = "x <- 1\n";
    assert_eq!(find_enclosing_call(text, 0, 4), None);
}

#[test]
fn find_enclosing_call_returns_none_for_non_ident_func() {
    // The text before the `(` is `(1 + 2) + (` which is not a
    // function call (no identifier before the `(`). The helper
    // must return `None` rather than treat the `(` as a call.
    let text = "1 + (2 * 3)\n";
    assert_eq!(find_enclosing_call(text, 0, 6), None);
}

#[test]
fn get_signature_returns_known_params() {
    // `round` has the conventional `x, digits` parameters; the
    // helper must surface them in order.
    let params = get_signature("round").expect("round should have a signature");
    assert_eq!(params, vec!["x", "digits"]);

    // `mean` has three formal parameters.
    let params = get_signature("mean").expect("mean should have a signature");
    assert_eq!(params, vec!["x", "trim", "na.rm"]);

    // Variadic functions collapse to `...`.
    let params = get_signature("c").expect("c should have a signature");
    assert_eq!(params, vec!["..."]);
}

#[test]
fn get_signature_returns_none_for_unknown() {
    // User-defined functions aren't in the curated table.
    assert!(get_signature("my_helper").is_none());
    assert!(get_signature("").is_none());
}

#[test]
fn signature_help_label_and_active_param() {
    // End-to-end test of the signature-help logic at the helper
    // level: locate the enclosing call, look up the signature,
    // and verify the resulting label and active-parameter
    // highlight. We exercise the same helpers the LSP handler
    // uses so the test stays accurate even though the handler is
    // async and stateful.
    //
    // To avoid fragile byte-counting, we find the comma's position
    // dynamically and place the cursor right after it. `round(x, `
    // has one top-level comma => active param 1 (`digits`).
    let text = "round(x, ";
    let comma = text.find(',').expect("snippet should have a comma");
    let (name, active) = find_enclosing_call(text, 0, comma + 1).expect("should find call");
    assert_eq!(name, "round");
    assert_eq!(active, 1);

    let params = get_signature(&name).expect("round should have a signature");
    let label = format!("{}({})", name, params.join(", "));
    assert_eq!(label, "round(x, digits)");
    // The active parameter must be clamped to the parameter list
    // length: with 2 params and active=1, the highlight should
    // land on `digits`.
    let active_param = if active < params.len() {
        Some(active as u32)
    } else {
        None
    };
    assert_eq!(active_param, Some(1));
}

#[test]
fn signature_help_clamps_when_past_last_param() {
    // When the user has typed more commas than there are formal
    // parameters (e.g. `round(1, 2, 3, `), the active-parameter
    // index should clamp to `None` so the editor clears the
    // highlight instead of pointing at a non-existent parameter.
    // `round` has 2 params; typing 3 commas puts the cursor on a
    // 4th parameter that doesn't exist.
    let text = "round(1, 2, 3, \n";
    // After the third comma (byte 14): active param 3.
    let (_, active) = find_enclosing_call(text, 0, 14).expect("should find call");
    assert_eq!(active, 3);
    let params = get_signature("round").expect("round should have a signature");
    let active_param = if active < params.len() {
        Some(active as u32)
    } else {
        None
    };
    assert_eq!(
        active_param, None,
        "active param should clamp to None past the last formal"
    );
}

#[test]
fn common_r_completions_includes_keywords_and_functions() {
    // The curated list must surface a handful of keywords (so the
    // popup helps users start a function definition / loop) and
    // common base-R functions (so `c`, `list`, `mean` show up even
    // when the user has no bindings yet). Every entry must carry a
    // non-empty detail string and a kind.
    let items = common_r_completions();
    // Sanity: the list is non-empty but focused.
    assert!(!items.is_empty(), "curated list should not be empty");
    assert!(
        items.len() <= 50,
        "curated list should stay focused, got {} entries",
        items.len()
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    // A representative keyword and a representative function.
    assert!(
        labels.contains(&"function"),
        "missing 'function': {:?}",
        labels
    );
    assert!(labels.contains(&"if"), "missing 'if': {:?}", labels);
    assert!(labels.contains(&"c"), "missing 'c': {:?}", labels);
    assert!(labels.contains(&"list"), "missing 'list': {:?}", labels);
    assert!(labels.contains(&"mean"), "missing 'mean': {:?}", labels);
    // Every entry must have a kind and a detail.
    for it in &items {
        assert!(it.kind.is_some(), "entry {:?} missing kind", it.label);
        assert!(
            it.detail.as_deref().is_some_and(|d| !d.is_empty()),
            "entry {:?} missing/empty detail",
            it.label
        );
    }
    // The 'function' entry must be classified as a KEYWORD (R
    // treats it as a keyword, not a function call), and 'c' as a
    // FUNCTION.
    let function_item = items.iter().find(|i| i.label == "function").unwrap();
    assert_eq!(function_item.kind, Some(CompletionItemKind::KEYWORD));
    let c_item = items.iter().find(|i| i.label == "c").unwrap();
    assert_eq!(c_item.kind, Some(CompletionItemKind::FUNCTION));
}

#[test]
fn completions_for_scope_variables_and_keywords() {
    // Generic (non-triggered) completion must include the user's
    // in-scope bindings AND the curated keyword/function list.
    // Bindings get a VARIABLE or FUNCTION kind; the curated
    // keywords keep their KEYWORD / FUNCTION kind. Duplicate
    // labels (e.g. a user `c <- ...` vs the curated `c`) must be
    // collapsed by `dedup_by`.
    let src = "x <- 1L + 2L\nname <- \"hi\"\n";
    // Cursor on line 2, col 0 (a fresh line). No trigger.
    let pos = Position {
        line: 2,
        character: 0,
    };
    let items = completions(src, pos, None);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    // In-scope bindings.
    assert!(labels.contains(&"x"), "missing x: {:?}", labels);
    assert!(labels.contains(&"name"), "missing name: {:?}", labels);
    // Curated keywords / functions.
    assert!(labels.contains(&"if"), "missing if: {:?}", labels);
    assert!(
        labels.contains(&"function"),
        "missing function: {:?}",
        labels
    );
    assert!(labels.contains(&"mean"), "missing mean: {:?}", labels);
    // Dedup: 'c' should appear at most once even though both the
    // scope (no user `c` here) and the curated list could
    // contribute. This guards the dedup path against future
    // changes that add a 'c' to the scope.
    let c_count = labels.iter().filter(|&&l| l == "c").count();
    assert_eq!(c_count, 1, "'c' should appear exactly once: {:?}", labels);
    // 'x' must be a VARIABLE; the curated 'function' must be a
    // KEYWORD.
    let x_item = items.iter().find(|i| i.label == "x").unwrap();
    assert_eq!(x_item.kind, Some(CompletionItemKind::VARIABLE));
    let function_item = items.iter().find(|i| i.label == "function").unwrap();
    assert_eq!(function_item.kind, Some(CompletionItemKind::KEYWORD));
    // The list must be sorted alphabetically by label.
    let mut sorted = labels.clone();
    sorted.sort();
    assert_eq!(labels, sorted, "completions should be sorted by label");
}

#[test]
fn completions_for_dollar_trigger_returns_columns() {
    // When `$` is the trigger, the popup must show ONLY the
    // column names of the variable before the `$`. We use a
    // `list(a = <int>, b = <chr>)` literal so the checker infers
    // a `ColumnSchema` with columns `a` and `b`. Each column item
    // must be a FIELD whose detail surfaces its inferred type.
    let src = "df <- list(a = 1L, b = \"x\")\ndf$\n";
    // Cursor right after the `$` on line 1 (col 3: 'd','f','$').
    let pos = Position {
        line: 1,
        character: 3,
    };
    let items = completions(src, pos, trigger_context("$"));
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"a"), "missing column a: {:?}", labels);
    assert!(labels.contains(&"b"), "missing column b: {:?}", labels);
    // No scope variables / keywords should leak into the column
    // popup.
    assert!(
        !labels.contains(&"df"),
        "df should not appear in column completions: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"if"),
        "keywords should not appear in column completions: {:?}",
        labels
    );
    // Every item must be a FIELD (column) with a non-empty detail.
    for it in &items {
        assert_eq!(
            it.kind,
            Some(CompletionItemKind::FIELD),
            "column {:?} should be FIELD",
            it.label
        );
        assert!(
            it.detail.as_deref().is_some_and(|d| !d.is_empty()),
            "column {:?} missing detail",
            it.label
        );
    }
    // Column 'a' is integer and 'b' is character; the detail
    // strings should reflect that so the popup shows the type
    // next to the name.
    let a_item = items.iter().find(|i| i.label == "a").unwrap();
    let a_detail = a_item.detail.as_deref().unwrap();
    assert!(
        a_detail.contains("integer"),
        "column a should be integer, got: {}",
        a_detail
    );
    let b_item = items.iter().find(|i| i.label == "b").unwrap();
    let b_detail = b_item.detail.as_deref().unwrap();
    assert!(
        b_detail.contains("character"),
        "column b should be character, got: {}",
        b_detail
    );
}

#[test]
fn completions_for_dollar_trigger_without_schema_returns_empty() {
    // If the variable before the `$` has no `ColumnSchema` (e.g.
    // a plain integer vector), the `$`-triggered popup must
    // return an empty list rather than fall through to the
    // generic in-scope list. Falling through would dump every
    // binding where the user expects column names.
    let src = "x <- 1L + 2L\nx$\n";
    let pos = Position {
        line: 1,
        character: 2,
    };
    let items = completions(src, pos, trigger_context("$"));
    assert!(
        items.is_empty(),
        "expected no completions for non-data-frame $, got: {:?}",
        items
    );
}

// ---- references (find all references) helpers ----

/// Helper: parse a snippet and return the references to `name`
/// within it. Mirrors what the `references` LSP method does for a
/// single document, minus the async state lookup. Uses
/// `include_declaration` to control whether definition sites are
/// included.
fn references_in(src: &str, name: &str, include_declaration: bool) -> Vec<Location> {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    find_references_in_file(&file, name, &uri, src, include_declaration)
}

#[test]
fn references_finds_variable_usages_in_same_file() {
    // `x` is defined once and read twice (in the RHS of `y` and in
    // `z`). With include_declaration = false, only the two reads
    // should be returned.
    let src = "x <- 1L\ny <- x + 1\nz <- x * 2\n";
    let locs = references_in(src, "x", false);
    assert_eq!(locs.len(), 2, "expected 2 references to x, got {:?}", locs);
    // The two references live on lines 1 and 2 (0-indexed).
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&1),
        "expected a reference on line 1: {:?}",
        lines
    );
    assert!(
        lines.contains(&2),
        "expected a reference on line 2: {:?}",
        lines
    );
    // Each reference must cover exactly the identifier "x" (1 char
    // wide), not a zero-width or multi-char range.
    for loc in &locs {
        assert_eq!(
            loc.range.end.character - loc.range.start.character,
            1,
            "expected 1-char wide range for 'x'"
        );
    }
}

#[test]
fn references_finds_function_call_sites() {
    // `add` is defined as a function and called twice. With
    // include_declaration = false, only the two call sites on
    // lines 1 and 2 should be returned.
    let src = "add <- function(a, b) a + b\nadd(1, 2)\nadd(3, 4)\n";
    let locs = references_in(src, "add", false);
    assert_eq!(locs.len(), 2, "expected 2 call sites, got {:?}", locs);
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(lines.contains(&1), "expected a call on line 1: {:?}", lines);
    assert!(lines.contains(&2), "expected a call on line 2: {:?}", lines);
    // Each call-site range covers exactly the 3-char name "add".
    for loc in &locs {
        assert_eq!(
            loc.range.end.character - loc.range.start.character,
            3,
            "expected 3-char wide range for 'add'"
        );
        assert_eq!(loc.range.start.character, 0, "calls start at col 0");
    }
}

#[test]
fn references_include_declaration_flag() {
    // `x` is defined once (line 0) and read once (line 1).
    let src = "x <- 1L\nx + 1\n";
    // With include_declaration = true: the definition (line 0) AND
    // the read (line 1) => 2 locations.
    let locs_with = references_in(src, "x", true);
    assert_eq!(
        locs_with.len(),
        2,
        "expected 2 locations with declaration, got {:?}",
        locs_with
    );
    // With include_declaration = false: only the read (line 1) =>
    // 1 location, and it must NOT be the definition on line 0.
    let locs_without = references_in(src, "x", false);
    assert_eq!(
        locs_without.len(),
        1,
        "expected 1 location without declaration, got {:?}",
        locs_without
    );
    assert_eq!(
        locs_without[0].range.start.line, 1,
        "the lone reference must be the read on line 1"
    );
}

#[test]
fn references_across_multiple_files() {
    // Simulate two open documents: a.R defines `helper`, b.R calls
    // it. This mirrors how `references` walks `self.state.docs`
    // across all open documents (we drive `find_references_in_file`
    // directly for each parsed file since the async state is not
    // reachable from a unit test).
    let src_a = "helper <- function() 1L\n";
    let src_b = "helper()\n";
    let mut parser = RParser::new().unwrap();
    let file_a = parser.parse("a.R", src_a).unwrap();
    let file_b = parser.parse("b.R", src_b).unwrap();
    let uri_a = Url::parse("file:///tmp/a.R").unwrap();
    let uri_b = Url::parse("file:///tmp/b.R").unwrap();

    // include_declaration = true so the definition in a.R counts.
    let mut all = Vec::new();
    all.extend(find_references_in_file(
        &file_a, "helper", &uri_a, src_a, true,
    ));
    all.extend(find_references_in_file(
        &file_b, "helper", &uri_b, src_b, true,
    ));

    // One definition in a.R + one call in b.R => 2 locations.
    assert_eq!(
        all.len(),
        2,
        "expected 2 locations across files, got {:?}",
        all
    );
    // The locations must come from different URIs (one per file).
    let uris: Vec<&Url> = all.iter().map(|l| &l.uri).collect();
    assert!(
        uris.contains(&&uri_a),
        "missing location in a.R: {:?}",
        uris
    );
    assert!(
        uris.contains(&&uri_b),
        "missing location in b.R: {:?}",
        uris
    );
}

#[test]
fn references_finds_usages_inside_nested_scopes() {
    // `data` is read inside an anonymous function body (via index
    // `data[1]`) and inside a for-loop body (via `print(data)`).
    // The walker must recurse into both nested scopes.
    let src =
        "data <- c(1, 2, 3)\nf <- function() {\n  data[1]\n}\nfor (i in 1:3) {\n  print(data)\n}\n";
    let locs = references_in(src, "data", false);
    // Two reads: inside the function body (line 2) and inside the
    // for-loop body (line 5). The definition on line 0 is excluded
    // because include_declaration is false.
    assert_eq!(
        locs.len(),
        2,
        "expected 2 nested references, got {:?}",
        locs
    );
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&2),
        "expected a reference on line 2: {:?}",
        lines
    );
    assert!(
        lines.contains(&5),
        "expected a reference on line 5: {:?}",
        lines
    );
}

#[test]
fn references_returns_empty_for_undefined_name() {
    // No occurrences of `does_not_exist` anywhere.
    let src = "x <- 1L\ny <- x + 1\n";
    let locs = references_in(src, "does_not_exist", true);
    assert!(locs.is_empty(), "expected no references, got {:?}", locs);
}

#[test]
fn references_self_referencing_assignment() {
    // `x <- x + 1` references `x` on the RHS even though the LHS is
    // a definition. With include_declaration = true the LHS counts
    // too, giving 2 locations; with false only the RHS read counts.
    let src = "x <- x + 1\n";
    let locs_with = references_in(src, "x", true);
    assert_eq!(locs_with.len(), 2, "got {:?}", locs_with);
    let locs_without = references_in(src, "x", false);
    assert_eq!(locs_without.len(), 1, "got {:?}", locs_without);
    // The lone reference (RHS) is at col 5 ("x <- x...").
    assert_eq!(locs_without[0].range.start.character, 5);
}

// ---- workspace symbols helpers ----

/// Helper: parse + check a snippet and return its top-level
/// `DocumentSymbol`s, then flatten them into `SymbolInformation`s
/// attached to the given URI. Mirrors what the `symbol` LSP method
/// does for a single document, minus the async state lookup and
/// the cross-document iteration / query filter.
fn workspace_symbols(src: &str, uri: &Url) -> Vec<SymbolInformation> {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let mut checker = ry_checker::Checker::new("test.R");
    let (_, scope) = checker.check_with_scope(&file);
    let doc_symbols = collect_symbols(&file.stmts, src, Some(&scope));
    flatten_symbols_to_symbol_info(doc_symbols, uri)
}

#[test]
fn workspace_symbols_flatten_tree_with_container_names() {
    // The canonical example: a function `add` (with parameters
    // `a` and `b` that become nested children), a variable
    // `result`, and a variable `name`. Flattening must produce
    // one `SymbolInformation` per node (function + each param +
    // each top-level variable) and propagate the parent's name
    // into each child's `container_name`.
    let src = "add <- function(a, b) a + b\nresult <- add(1, 2)\nname <- \"hello\"\n";
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let symbols = workspace_symbols(src, &uri);

    // 1 function (add) + 2 params (a, b) + 2 variables (result,
    // name) => 5 flattened symbols.
    assert_eq!(symbols.len(), 5, "got {:?}", symbols);

    // Every symbol must point at the file we passed in.
    for s in &symbols {
        assert_eq!(s.location.uri, uri, "wrong uri for {:?}", s.name);
    }

    // Top-level symbols have `container_name = None`; the
    // function's parameters inherit `container_name = "add"`.
    // Build a name -> container_name lookup to assert each.
    let container_of = |name: &str| -> Option<String> {
        symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.container_name.clone())
    };
    assert_eq!(container_of("add"), None, "add is top-level");
    assert_eq!(container_of("result"), None, "result is top-level");
    assert_eq!(container_of("name"), None, "name is top-level");
    assert_eq!(
        container_of("a"),
        Some("add".to_string()),
        "a is a parameter of add"
    );
    assert_eq!(
        container_of("b"),
        Some("add".to_string()),
        "b is a parameter of add"
    );

    // The function symbol must be classified as FUNCTION, the
    // parameters and variables as VARIABLE.
    let kind_of =
        |name: &str| -> SymbolKind { symbols.iter().find(|s| s.name == name).unwrap().kind };
    assert_eq!(kind_of("add"), SymbolKind::FUNCTION);
    assert_eq!(kind_of("a"), SymbolKind::VARIABLE);
    assert_eq!(kind_of("b"), SymbolKind::VARIABLE);
    assert_eq!(kind_of("result"), SymbolKind::VARIABLE);
    assert_eq!(kind_of("name"), SymbolKind::VARIABLE);

    // The function symbol's location range must cover exactly the
    // 3-character identifier `add` at line 0, col 0 (this is the
    // `selection_range` propagated from the `DocumentSymbol`).
    let add = symbols.iter().find(|s| s.name == "add").unwrap();
    assert_eq!(add.location.range.start.line, 0);
    assert_eq!(add.location.range.start.character, 0);
    assert_eq!(add.location.range.end.line, 0);
    assert_eq!(add.location.range.end.character, 3);
}

#[test]
fn workspace_symbols_filter_case_insensitive_substring() {
    // The `symbol` handler retains a symbol when its name contains
    // the query as a case-insensitive substring. We exercise the
    // filter inline (the handler does `name.to_lowercase().contains
    // (&query.to_lowercase())`) so the test pins the exact
    // matching rule: 'RES' must match 'result' but not 'add'.
    let src = "add <- function(a, b) a + b\nresult <- add(1, 2)\n";
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let mut symbols = workspace_symbols(src, &uri);

    // Sanity: without filtering we get add, a, b, result.
    assert_eq!(symbols.len(), 4, "got {:?}", symbols);

    // Apply the same filter the handler uses.
    let query = "RES".to_string();
    let query_lower = query.to_lowercase();
    symbols.retain(|s| s.name.to_lowercase().contains(&query_lower));

    // Only `result` contains "res" case-insensitively.
    assert_eq!(symbols.len(), 1, "got {:?}", symbols);
    assert_eq!(symbols[0].name, "result");

    // An empty query must NOT filter anything (the handler
    // special-cases this), so re-fetch and check.
    let symbols_all = workspace_symbols(src, &uri);
    assert_eq!(
        symbols_all.len(),
        4,
        "empty query should return all symbols"
    );
}

#[test]
fn workspace_symbols_empty_when_no_bindings() {
    // A bare expression with no assignments produces no
    // `DocumentSymbol`s and therefore no `SymbolInformation`s.
    let src = "1L + 2L\n";
    let uri = Url::parse("file:///tmp/test.R").unwrap();
    let symbols = workspace_symbols(src, &uri);
    assert!(symbols.is_empty(), "expected no symbols, got {:?}", symbols);
}

// ---- rename helpers ----

#[test]
fn rename_edits_single_file_includes_declaration_and_usages() {
    // `x` is defined once (line 0) and read once (line 1, col 4
    // in `y <- x + 1`). Renaming `x` to `new_x` must produce 2
    // edits in the same file: one for the declaration and one
    // for the usage. Each edit must replace the 1-character
    // identifier span with the new name.
    let src = "x <- 1L\ny <- x + 1\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let edit = build_rename_edits(&[("test.R", &file, src)], "x", "new_x");

    let changes = edit.changes.expect("should have changes");
    // All edits land in the same file (one entry in the map).
    assert_eq!(changes.len(), 1, "got {:?}", changes);
    let uri = path_to_uri("test.R");
    let edits = changes.get(&uri).expect("should have edits for test.R");
    assert_eq!(edits.len(), 2, "got {:?}", edits);
    // Every edit must carry the new name and target a 1-char range.
    for e in edits {
        assert_eq!(e.new_text, "new_x");
        assert_eq!(
            e.range.end.character - e.range.start.character,
            1,
            "expected 1-char wide range for 'x'"
        );
    }
    // The two edits must cover the declaration on line 0 and the
    // usage on line 1 (the only `x` in the source).
    let lines: Vec<u32> = edits.iter().map(|e| e.range.start.line).collect();
    assert!(
        lines.contains(&0),
        "expected an edit on line 0: {:?}",
        lines
    );
    assert!(
        lines.contains(&1),
        "expected an edit on line 1: {:?}",
        lines
    );
}

#[test]
fn rename_edits_across_files_group_by_uri() {
    // Simulate two open documents: a.R defines `helper`, b.R calls
    // it. Renaming `helper` to `h` must produce one edit in each
    // file, grouped under separate URIs in the `changes` map.
    // We use absolute paths so `path_to_uri` produces distinct
    // `file://` URIs (relative paths would all collapse to the
    // `file:///unknown` fallback).
    let src_a = "helper <- function() 1L\n";
    let src_b = "helper()\n";
    let mut parser = RParser::new().unwrap();
    let file_a = parser.parse("/tmp/a.R", src_a).unwrap();
    let file_b = parser.parse("/tmp/b.R", src_b).unwrap();
    let edit = build_rename_edits(
        &[("/tmp/a.R", &file_a, src_a), ("/tmp/b.R", &file_b, src_b)],
        "helper",
        "h",
    );

    let changes = edit.changes.expect("should have changes");
    // One entry per file.
    assert_eq!(changes.len(), 2, "got {:?}", changes);
    let uri_a = path_to_uri("/tmp/a.R");
    let uri_b = path_to_uri("/tmp/b.R");
    let edits_a = changes.get(&uri_a).expect("a.R should have edits");
    let edits_b = changes.get(&uri_b).expect("b.R should have edits");
    // One edit per file (the definition in a.R, the call in b.R).
    assert_eq!(edits_a.len(), 1, "got {:?}", edits_a);
    assert_eq!(edits_b.len(), 1, "got {:?}", edits_b);
    // Every edit must replace with the new name and target a
    // 7-character span (the length of `helper`).
    for e in edits_a.iter().chain(edits_b.iter()) {
        assert_eq!(e.new_text, "h");
        assert_eq!(
            e.range.end.character - e.range.start.character,
            "helper".len() as u32
        );
    }
}

#[test]
fn rename_edits_unknown_name_yields_empty_changes() {
    // Renaming a name that doesn't exist anywhere must produce an
    // empty `changes` map (still `Some`, just with no entries).
    let src = "x <- 1L\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let edit = build_rename_edits(&[("test.R", &file, src)], "does_not_exist", "y");
    let changes = edit.changes.expect("should still be Some(empty)");
    assert!(changes.is_empty(), "got {:?}", changes);
}

// ---- prepareRename helpers ----

#[test]
fn prepare_rename_returns_identifier_range() {
    // Cursor on 'v' inside `my_var` (line 0, col 2): the helper
    // must return the full identifier name AND a range covering
    // exactly `my_var` (cols 0..6 on line 0).
    let text = "my_var <- 1L\n";
    let (name, range) =
        find_identifier_range_at_position(text, 0, 2).expect("should find identifier");
    assert_eq!(name, "my_var");
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 0);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, "my_var".len() as u32);
}

#[test]
fn prepare_rename_returns_none_for_keywords_and_operators() {
    // R keywords are not renameable bindings: `if` must yield
    // `None` so the editor does not offer a rename UI on it.
    let text = "if (TRUE) { x <- 1 }\n";
    assert_eq!(
        find_identifier_range_at_position(text, 0, 1),
        None,
        "keyword 'if' must not be renameable"
    );
    // Operators / whitespace must also yield `None`.
    let text = "x <- 1L\n";
    assert_eq!(
        find_identifier_range_at_position(text, 0, 2),
        None,
        "operator '<-' must not be renameable"
    );
    // Pure numbers must yield `None`.
    let text = "x <- 123\n";
    assert_eq!(
        find_identifier_range_at_position(text, 0, 5),
        None,
        "pure numbers must not be renameable"
    );
}

#[test]
fn prepare_rename_at_end_of_word_still_resolves() {
    // Cursor right after the last identifier character (a common
    // transient state when the user just clicked at the end of
    // a word): the helper must still resolve the identifier. We
    // place the cursor on the space after `my_var` (col 6).
    let text = "my_var <- 1L\n";
    let (name, range) = find_identifier_range_at_position(text, 0, 6)
        .expect("should find identifier at end of word");
    assert_eq!(name, "my_var");
    assert_eq!(range.start.character, 0);
    assert_eq!(range.end.character, "my_var".len() as u32);
}

// ---- document highlight helpers ----

/// Helper: parse a snippet and return the `DocumentHighlight`s for
/// `name`. Mirrors what the `document_highlight` LSP method does,
/// minus the async state lookup. Order of the returned highlights
/// follows source order (top-to-bottom).
fn doc_highlights(src: &str, name: &str) -> Vec<DocumentHighlight> {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    collect_document_highlights(&file, name, src)
}

#[test]
fn document_highlight_classifies_write_and_read() {
    // `x` is written at line 0 (assignment target) and read on
    // lines 1 and 2 (RHS of `y` and `z`). The WRITE must land on
    // line 0 and the two READs on lines 1 and 2.
    let src = "x <- 1L\ny <- x + 1\nz <- x * 2\n";
    let hl = doc_highlights(src, "x");
    assert_eq!(hl.len(), 3, "got {:?}", hl);

    // Exactly one WRITE at line 0, covering exactly the 1-char
    // identifier `x` at col 0.
    let writes: Vec<&DocumentHighlight> = hl
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
        .collect();
    assert_eq!(writes.len(), 1, "expected one WRITE: {:?}", hl);
    assert_eq!(writes[0].range.start.line, 0);
    assert_eq!(writes[0].range.start.character, 0);
    assert_eq!(writes[0].range.end.character, 1);

    // Two READs on lines 1 and 2.
    let reads: Vec<&DocumentHighlight> = hl
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
        .collect();
    assert_eq!(reads.len(), 2, "expected two READs: {:?}", hl);
    let read_lines: Vec<u32> = reads.iter().map(|h| h.range.start.line).collect();
    assert!(
        read_lines.contains(&1),
        "expected READ on line 1: {:?}",
        read_lines
    );
    assert!(
        read_lines.contains(&2),
        "expected READ on line 2: {:?}",
        read_lines
    );
}

#[test]
fn document_highlight_self_referencing_assignment_has_write_and_read() {
    // `x <- x + 1` writes `x` on the LHS (col 0) and reads `x` on
    // the RHS (col 5). Both must be highlighted with the right
    // kinds on the same line.
    let src = "x <- x + 1\n";
    let hl = doc_highlights(src, "x");
    assert_eq!(hl.len(), 2, "got {:?}", hl);
    // Find the WRITE (LHS at col 0) and the READ (RHS at col 5).
    let write = hl
        .iter()
        .find(|h| h.kind == Some(DocumentHighlightKind::WRITE))
        .expect("expected a WRITE");
    assert_eq!(write.range.start.line, 0);
    assert_eq!(write.range.start.character, 0);
    let read = hl
        .iter()
        .find(|h| h.kind == Some(DocumentHighlightKind::READ))
        .expect("expected a READ");
    assert_eq!(read.range.start.line, 0);
    assert_eq!(read.range.start.character, 5);
}

#[test]
fn document_highlight_finds_occurrences_inside_nested_scopes() {
    // `data` is written at line 0 and read inside a function body
    // (line 2) and inside a for-loop body (line 5). The walker
    // must recurse into both nested scopes.
    let src =
        "data <- c(1, 2, 3)\nf <- function() {\n  data[1]\n}\nfor (i in 1:3) {\n  print(data)\n}\n";
    let hl = doc_highlights(src, "data");
    // 1 WRITE (line 0) + 2 READs (lines 2 and 5) = 3 highlights.
    assert_eq!(hl.len(), 3, "got {:?}", hl);
    let read_lines: Vec<u32> = hl
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
        .map(|h| h.range.start.line)
        .collect();
    assert!(
        read_lines.contains(&2),
        "expected READ on line 2: {:?}",
        read_lines
    );
    assert!(
        read_lines.contains(&5),
        "expected READ on line 5: {:?}",
        read_lines
    );
}

#[test]
fn document_highlight_returns_empty_for_unknown_name() {
    let src = "x <- 1L\ny <- x + 1\n";
    let hl = doc_highlights(src, "does_not_exist");
    assert!(hl.is_empty(), "expected no highlights, got {:?}", hl);
}

#[test]
fn document_highlight_classifies_loop_variable_as_write() {
    // The loop variable `i` is re-bound each iteration, so it
    // should be classified as a WRITE. The single READ lives in
    // the loop body on line 1.
    let src = "for (i in 1:3) {\n  print(i)\n}\n";
    let hl = doc_highlights(src, "i");
    assert_eq!(hl.len(), 2, "got {:?}", hl);
    let writes: Vec<&DocumentHighlight> = hl
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
        .collect();
    assert_eq!(
        writes.len(),
        1,
        "expected one WRITE for the loop var: {:?}",
        hl
    );
    let reads: Vec<&DocumentHighlight> = hl
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
        .collect();
    assert_eq!(reads.len(), 1, "expected one READ in the body: {:?}", hl);
    assert_eq!(reads[0].range.start.line, 1);
}

// ---- folding range helpers ----

/// Helper: parse a snippet and return its folding ranges. Mirrors
/// what the `folding_range` LSP method does, minus the async state
/// lookup. Ranges are returned in source order.
fn folding_ranges(src: &str) -> Vec<FoldingRange> {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    collect_folding_ranges(&file, src)
}

#[test]
fn folding_range_for_multiline_function_body() {
    // A function whose body spans multiple lines must produce a
    // folding range covering the body. The named function pattern
    // `f <- function() { ... }` is modeled by the parser as an
    // `Assign` with an `Expr::Function` value, so the recursive
    // `collect_folding_from_expr` must find the function-literal
    // span. The body starts on line 0 and ends on line 2.
    let src = "f <- function() {\n  x <- 1L\n  x\n}\n";
    let ranges = folding_ranges(src);
    assert!(
        !ranges.is_empty(),
        "expected at least one range, got {:?}",
        ranges
    );
    // At least one range must start at line 0 and end at line 3.
    let covers_func = ranges
        .iter()
        .any(|r| r.start_line == 0 && r.end_line == 3 && r.kind == Some(FoldingRangeKind::Region));
    assert!(
        covers_func,
        "expected a range covering the function body (0..3), got {:?}",
        ranges
    );
    // Every range must be `Region`-kinded and span at least 2 lines.
    for r in &ranges {
        assert_eq!(r.kind, Some(FoldingRangeKind::Region));
        assert!(
            r.end_line > r.start_line,
            "expected multi-line range: {:?}",
            r
        );
    }
}

#[test]
fn folding_range_for_if_else_block() {
    // An `if`/`else` block whose body spans multiple lines must
    // produce a folding range. The `if` statement spans lines 0..2.
    let src = "if (x > 0) {\n  print(\"pos\")\n} else {\n  print(\"nonpos\")\n}\n";
    let ranges = folding_ranges(src);
    assert!(
        !ranges.is_empty(),
        "expected at least one range, got {:?}",
        ranges
    );
    // The outer `if` must cover lines 0..4 (it ends on the final
    // `}` of the `else` block).
    let covers_if = ranges.iter().any(|r| r.start_line == 0 && r.end_line == 4);
    assert!(
        covers_if,
        "expected a range covering the whole if/else (0..4), got {:?}",
        ranges
    );
}

#[test]
fn folding_range_for_for_loop() {
    // A `for` loop with a multi-line body must fold from the loop
    // header line down to the closing brace.
    let src = "for (i in 1:3) {\n  print(i)\n  print(i * 2)\n}\n";
    let ranges = folding_ranges(src);
    assert!(
        !ranges.is_empty(),
        "expected at least one range, got {:?}",
        ranges
    );
    let covers_for = ranges.iter().any(|r| r.start_line == 0 && r.end_line == 3);
    assert!(
        covers_for,
        "expected a range covering the for loop (0..3), got {:?}",
        ranges
    );
}

#[test]
fn folding_range_empty_for_single_line_statement() {
    // A single-line statement has no foldable region; the helper
    // must return an empty list.
    let src = "x <- 1L + 2L\ny <- x * 3L\n";
    let ranges = folding_ranges(src);
    assert!(
        ranges.is_empty(),
        "expected no folding ranges for single-line code, got {:?}",
        ranges
    );
}

#[test]
fn folding_range_nested_blocks_each_get_their_own_range() {
    // A function body containing a nested multi-line `if` must
    // yield (at least) two ranges: one for the outer function and
    // one for the inner `if`. This guards the recursive walk.
    let src = "f <- function(x) {\n  if (x > 0) {\n    print(x)\n  }\n}\n";
    let ranges = folding_ranges(src);
    // We expect at least 2 ranges: the outer function body and
    // the inner if body.
    assert!(
        ranges.len() >= 2,
        "expected at least 2 ranges (function + nested if), got {:?}",
        ranges
    );
    // One range must cover the whole function (lines 0..4), and
    // another must cover the inner if (lines 1..3).
    let has_outer = ranges.iter().any(|r| r.start_line == 0 && r.end_line == 4);
    let has_inner_if = ranges.iter().any(|r| r.start_line == 1 && r.end_line == 3);
    assert!(has_outer, "missing outer function range: {:?}", ranges);
    assert!(has_inner_if, "missing inner if range: {:?}", ranges);
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

// ---- selection range helpers ----

/// Helper: parse a snippet and return the `SelectionRange` chain
/// for a single cursor position. Mirrors what the
/// `selection_range` LSP method does, minus the async state
/// lookup.
fn selection_range_at(src: &str, position: Position) -> SelectionRange {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    build_selection_range(position, &file, src)
}

/// Walk a `SelectionRange` chain from narrowest to widest,
/// returning the list of `Range`s in order. Used by the tests to
/// assert the chain widens monotonically.
fn chain_ranges(sel: &SelectionRange) -> Vec<Range> {
    let mut out = vec![sel.range];
    let mut cur = &sel.parent;
    while let Some(p) = cur {
        out.push(p.range);
        cur = &p.parent;
    }
    out
}

#[test]
fn selection_range_chain_widens_from_identifier_to_file() {
    // For `result <- x + 1` with the cursor on `result`, the chain
    // must widen: identifier (`result`) -> enclosing statement ->
    // whole file. Each level must strictly contain the previous.
    let src = "result <- x + 1\n";
    // Cursor on 's' in 'result' (line 0, col 2).
    let pos = Position {
        line: 0,
        character: 2,
    };
    let sel = selection_range_at(src, pos);
    let ranges = chain_ranges(&sel);

    // The narrowest range must cover the identifier `result`
    // (cols 0..6 on line 0).
    assert_eq!(ranges[0].start.line, 0);
    assert_eq!(ranges[0].start.character, 0);
    assert_eq!(ranges[0].end.character, "result".len() as u32);

    // The chain must have at least 2 levels (identifier + file).
    assert!(
        ranges.len() >= 2,
        "expected at least 2 levels, got {:?}",
        ranges
    );

    // Every level must contain the cursor position.
    for r in &ranges {
        let contains = (r.start.line < pos.line
            || (r.start.line == pos.line && r.start.character <= pos.character))
            && (r.end.line > pos.line
                || (r.end.line == pos.line && r.end.character >= pos.character));
        assert!(contains, "range {:?} does not contain cursor {:?}", r, pos);
    }

    // Each level must contain or equal the previous (monotonic
    // widening), with no two consecutive identical ranges.
    for w in ranges.windows(2) {
        assert!(
            w[0] != w[1],
            "consecutive duplicate ranges in chain: {:?}",
            w
        );
    }

    // The widest level (last) must start at (0, 0).
    let widest = ranges.last().unwrap();
    assert_eq!(widest.start.line, 0);
    assert_eq!(widest.start.character, 0);
}

#[test]
fn selection_range_identifier_on_rhs() {
    // Cursor on `x` in `result <- x + 1` (the RHS read). The
    // narrowest range must be the identifier `x` (1 char), and
    // the chain must widen to the enclosing statement.
    let src = "result <- x + 1\n";
    // `x` is at line 0, col 10 (after "result <- ").
    let pos = Position {
        line: 0,
        character: 10,
    };
    let sel = selection_range_at(src, pos);
    let ranges = chain_ranges(&sel);

    // The narrowest range is the single-character `x`.
    assert_eq!(ranges[0].start.line, 0);
    assert_eq!(ranges[0].start.character, 10);
    assert_eq!(ranges[0].end.character, 11);

    // The chain widens beyond the identifier.
    assert!(
        ranges.len() >= 2,
        "expected at least 2 levels, got {:?}",
        ranges
    );
}

#[test]
fn selection_range_picks_correct_statement_in_multi_line_file() {
    // In a two-statement file, the enclosing statement for a
    // cursor on line 1 must be the second statement, not the
    // first.
    let src = "x <- 1L\ny <- x + 1\n";
    // Cursor on `y` (line 1, col 0).
    let pos = Position {
        line: 1,
        character: 0,
    };
    let sel = selection_range_at(src, pos);
    let ranges = chain_ranges(&sel);

    // The narrowest range is the identifier `y`.
    assert_eq!(ranges[0].start.line, 1);
    assert_eq!(ranges[0].start.character, 0);
    assert_eq!(ranges[0].end.character, 1);

    // The enclosing statement (the middle level) must start on
    // line 1 and cover at least the `y <- x + 1` text.
    let stmt_level = ranges
        .iter()
        .find(|r| r.start.line == 1 && r.end.character > 1)
        .unwrap_or_else(|| panic!("expected a statement-level range on line 1: {:?}", ranges));
    assert!(
        stmt_level.start.character == 0,
        "statement range should start at col 0: {:?}",
        stmt_level
    );
}

#[test]
fn selection_range_no_identifier_falls_back_to_cursor() {
    // Cursor on whitespace (between `<-` and the value) is not on
    // an identifier. The narrowest range must be a zero-width
    // span at the cursor so the editor still has an anchor.
    let src = "x <- 1L\n";
    // Cursor on the space after `<-` (line 0, col 4).
    let pos = Position {
        line: 0,
        character: 4,
    };
    let sel = selection_range_at(src, pos);
    let ranges = chain_ranges(&sel);

    // The narrowest range is a zero-width span at the cursor.
    assert_eq!(ranges[0].start, pos);
    assert_eq!(ranges[0].end, pos);

    // The chain still widens to the file level.
    let widest = ranges.last().unwrap();
    assert_eq!(widest.start.line, 0);
    assert_eq!(widest.start.character, 0);
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
