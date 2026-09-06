//! Parser/checker invariants.
//!
//! These tests deliberately exercise the public parser and checker seams. They do not
//! reconstruct tree-sitter's grammar or the checker's diagnostic selection.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ry_checker::Checker;
use ry_core::ast::Stmt;
use ry_core::{RParser, Span};

const CHECKER_FIXTURE_FLOOR: usize = 229;
const ECOSYSTEM_SAMPLE_MODULUS: u64 = 5;

fn r_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut pending = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(next) = pending.pop() {
        for entry in
            fs::read_dir(&next).unwrap_or_else(|error| panic!("read {}: {error}", next.display()))
        {
            let path = entry.expect("directory entry").path();
            if path.is_dir() && recursive {
                pending.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("R") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn checker_fixtures() -> Vec<PathBuf> {
    r_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata"),
        false,
    )
}

/// A stable FNV-1a selection keeps the ecosystem net deterministic without making
/// its membership depend on directory enumeration order.
fn ecosystem_sample() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/vendor");
    r_files(&root, true)
        .into_iter()
        .filter(|path| {
            let relative = path.strip_prefix(&root).expect("vendor-relative path");
            stable_hash(relative.to_string_lossy().as_bytes())
                .is_multiple_of(ECOSYSTEM_SAMPLE_MODULUS)
        })
        .collect()
}

fn stable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn assert_valid_span(path: &Path, src: &str, code: &str, span: Span) {
    assert!(
        span.start <= span.end,
        "{} {code}: reversed diagnostic span {span:?}",
        path.display()
    );
    assert!(
        span.end <= src.len(),
        "{} {code}: diagnostic span {span:?} exceeds {} source bytes",
        path.display(),
        src.len()
    );
    assert!(
        src.is_char_boundary(span.start),
        "{} {code}: diagnostic start is not a UTF-8 boundary: {span:?}",
        path.display()
    );
    assert!(
        src.is_char_boundary(span.end),
        "{} {code}: diagnostic end is not a UTF-8 boundary: {span:?}",
        path.display()
    );
}

fn check_r1(paths: &[PathBuf]) {
    let mut parser = RParser::new().expect("parser init");
    for path in paths {
        let src = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let name = path.to_string_lossy();
        let file = parser
            .parse(&name, &src)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        let mut checker = Checker::new(&name);
        checker.check(&file);
        for diagnostic in checker.take_diagnostics() {
            assert_valid_span(path, &src, diagnostic.code, diagnostic.span);
        }
    }
}

#[test]
fn r1_all_checker_fixture_diagnostic_spans_are_valid() {
    let fixtures = checker_fixtures();
    assert!(
        fixtures.len() >= CHECKER_FIXTURE_FLOOR,
        "expected at least {CHECKER_FIXTURE_FLOOR} checker fixtures, found {}",
        fixtures.len()
    );
    check_r1(&fixtures);
}

#[test]
fn r1_deterministic_ecosystem_sample_diagnostic_spans_are_valid() {
    let sample = ecosystem_sample();
    assert!(
        sample.len() >= 10,
        "ecosystem sample unexpectedly small: {} files",
        sample.len()
    );
    check_r1(&sample);
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Assign { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::FunctionDef { span, .. }
        | Stmt::Return { span, .. } => *span,
        Stmt::Expr(expr) => expr_span(expr),
    }
}

fn expr_span(expr: &ry_core::ast::Expr) -> Span {
    use ry_core::ast::Expr;
    match expr {
        Expr::Logical(_, span)
        | Expr::Integer(_, span)
        | Expr::Double(_, span)
        | Expr::String(_, span)
        | Expr::Null(span)
        | Expr::Na(_, span)
        | Expr::Unknown(span) => *span,
        Expr::Call { span, .. }
        | Expr::Ident { span, .. }
        | Expr::BinOp { span, .. }
        | Expr::UnaryOp { span, .. }
        | Expr::Index { span, .. }
        | Expr::Function { span, .. }
        | Expr::Block { span, .. }
        | Expr::If { span, .. } => *span,
    }
}

fn contains_statement_span(stmts: &[Stmt], expected: Span) -> bool {
    stmts.iter().any(|stmt| {
        if stmt_span(stmt).start == expected.start && stmt_span(stmt).end == expected.end {
            return true;
        }
        match stmt {
            Stmt::If { then, else_, .. } => {
                contains_statement_span(then, expected)
                    || else_
                        .as_deref()
                        .is_some_and(|branch| contains_statement_span(branch, expected))
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::FunctionDef { body, .. } => {
                contains_statement_span(body, expected)
            }
            Stmt::Expr(ry_core::ast::Expr::Block { body, .. }) => {
                contains_statement_span(body, expected)
            }
            _ => false,
        }
    })
}

fn parseable_statement_slices(paths: &[PathBuf]) -> BTreeSet<String> {
    let mut parser = RParser::new().expect("parser init");
    let mut candidates = BTreeSet::new();
    for path in paths {
        let src = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let file = parser
            .parse(&path.to_string_lossy(), &src)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        for stmt in &file.stmts {
            let span = stmt_span(stmt);
            let Some(candidate) = src.get(span.start..span.end) else {
                panic!("{} has invalid statement span {span:?}", path.display());
            };
            let standalone = parser
                .parse("candidate.R", candidate)
                .expect("parse candidate");
            if !candidate.trim_start().starts_with('#')
                && standalone.parse_errors.is_empty()
                && standalone.stmts.len() == 1
            {
                candidates.insert(candidate.to_string());
            }
        }
    }
    candidates
}

fn assert_statement_survives_at_every_k(candidate: &str) {
    const HOST: [&str; 2] = ["r6_before <- TRUE", "r6_after <- FALSE"];
    for k in 0..=HOST.len() {
        let mut pieces: Vec<&str> = HOST.to_vec();
        pieces.insert(k, candidate);
        let src = pieces.join("\n");
        let start = pieces[..k].iter().map(|part| part.len() + 1).sum::<usize>();
        let expected = Span::new(start, start + candidate.len(), 0, 0);

        let mut parser = RParser::new().expect("parser init");
        let file = parser.parse("r6.R", &src).expect("parse insertion host");
        let represented = contains_statement_span(&file.stmts, expected);
        let explicitly_rejected = file
            .parse_errors
            .iter()
            .any(|span| span.start <= expected.end && span.end >= expected.start);
        assert!(
            represented || explicitly_rejected,
            "statement disappeared at k={k} without a parse diagnostic:\n{candidate}\nAST: {:#?}\nparse errors: {:?}",
            file.stmts,
            file.parse_errors
        );

        // Exercise the lowering path for a brace used as a statement inside
        // another brace. Historically `lower_braced_as_stmt` kept only its
        // last child and even overwrote a previously lowered child with None.
        let nested_body = pieces.join("\n");
        let nested_src = format!("{{\n{{\n{nested_body}\n}}\n}}\n");
        let nested_start = 4 + start;
        let nested_expected = Span::new(nested_start, nested_start + candidate.len(), 0, 0);
        let nested = parser
            .parse("r6-nested.R", &nested_src)
            .expect("parse nested insertion host");
        let represented = contains_statement_span(&nested.stmts, nested_expected);
        let explicitly_rejected = nested
            .parse_errors
            .iter()
            .any(|span| span.start <= nested_expected.end && span.end >= nested_expected.start);
        assert!(
            represented || explicitly_rejected,
            "nested statement disappeared at k={k} without a parse diagnostic:\n{candidate}\nAST: {:#?}\nparse errors: {:?}",
            nested.stmts,
            nested.parse_errors
        );
    }
}

#[test]
fn r6_parseable_statements_from_checker_and_ecosystem_corpora_survive_insertion() {
    let checker = checker_fixtures();
    assert!(checker.len() >= CHECKER_FIXTURE_FLOOR);
    let ecosystem = ecosystem_sample();
    assert!(ecosystem.len() >= 10);

    let mut candidates = parseable_statement_slices(&checker);
    candidates.extend(parseable_statement_slices(&ecosystem));
    // Historical regressions: Rust's numeric parser rejects R hex numerics. The
    // old integer and float arms returned `None`, which deleted the statement.
    candidates.insert("0x1p2L".to_string());
    candidates.insert("0x1.8p2".to_string());

    assert!(
        candidates.len() >= 100,
        "statement corpus unexpectedly small: {} candidates",
        candidates.len()
    );
    for candidate in candidates {
        assert_statement_survives_at_every_k(&candidate);
    }
}

// ── fuzz-found parser regressions ──────────────────────────────

/// Regression fixture promoted from the `parse` cargo-fuzz target.
///
/// The minimized crash input `n"\xff` (3 bytes) caused the parser to panic
/// in `unquote_r_string`: slicing `&raw[1..raw.len() - 1]` landed inside
/// the U+FFFD replacement character when a malformed string literal ended
/// on a multi-byte char boundary. The fix walks back to the nearest char
/// boundary before slicing. This test locks in the fix.
#[test]
fn fuzz_regression_unquote_does_not_panic_on_non_char_boundary() {
    // The raw fuzz bytes: b'n"\xff' -- invalid UTF-8 that from_utf8_lossy
    // turns into `n"<replacement char>`.
    let bytes: &[u8] = b"n\"\xff";
    let src = String::from_utf8_lossy(bytes);
    let mut parser = RParser::new().expect("parser init");
    // Must not panic.
    let file = parser.parse("fuzz_regression.R", &src).expect("parse");
    // R1: every parse-error span is valid even on malformed input.
    for span in &file.parse_errors {
        assert!(span.start <= span.end);
        assert!(span.end <= src.len());
        assert!(src.is_char_boundary(span.start));
        assert!(src.is_char_boundary(span.end));
    }
}
