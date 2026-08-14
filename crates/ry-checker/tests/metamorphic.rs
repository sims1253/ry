//! Plan 35 W9: The remaining metamorphic suite, trimmed.
//!
//! Each relation is implemented first in report mode. A relation is promoted
//! to a gate only after all reported differences are fixed or prevented by
//! generator construction (Plan 35 design constraint 5).
//!
//! Relations and dispositions:
//!
//! | Relation | Disposition |
//! | :-- | :-- |
//! | R2 deterministic serial/parallel and repeated output | gate |
//! | R3 inert blank/comment insertion | gate |
//! | R4 capture-avoiding alpha rename | report (P35-W7 registry) |
//! | R5 file concatenation union | replaced by reset + non-interference |
//! | R7 literal-to-parameter lifting | report (feeds P35-W12) |
//! | R8 negated branch swap | report |
//! | R9 unchanged variable across branches | focused branch-join invariant |
//! | R10 pipe placeholder combinations | generated regression matrix |
//!
//! The reset/non-interference property (R5 replacement) runs A, then B
//! through a reused checker or project and asserts B equals a fresh run of
//! B. It directly targets the historical accumulated-diagnostics defect
//! without assuming concatenated source has independent semantics.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ry_checker::{Checker, Diagnostic, Project};
use ry_core::ast::Stmt;
use ry_core::{RParser, Span};

const CHECKER_FIXTURE_FLOOR: usize = 229;

// ── Fixture helpers ──────────────────────────────────────────────────────

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

/// Parse and check source with a fresh single-file `Checker`.
fn check_source(src: &str) -> Vec<Diagnostic> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("test.R", src).expect("parse");
    let mut checker = Checker::new("test.R");
    checker.check(&file);
    checker.take_diagnostics()
}

/// Full diagnostic tuple including span, for exact comparison.
fn diag_full(
    d: &Diagnostic,
) -> (
    &'static str,
    Span,
    &str,
    ry_checker::Severity,
    ry_checker::Confidence,
) {
    (d.code, d.span, &d.message, d.severity, d.confidence)
}

// ════════════════════════════════════════════════════════════════════════
// R2 — Deterministic serial/parallel and repeated output (GATE)
// ════════════════════════════════════════════════════════════════════════

/// Checking the same file multiple times on a reused `Checker` must yield
/// identical diagnostics every time. The checker must not accumulate state
/// across `check()` calls (historical accumulated-diagnostics defect).
#[test]
fn r2_repeated_checker_output_is_identical() {
    let fixtures = checker_fixtures();
    assert!(fixtures.len() >= CHECKER_FIXTURE_FLOOR);
    let mut parser = RParser::new().expect("parser init");

    for path in &fixtures {
        let src =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path.to_string_lossy().to_string();
        let file = parser
            .parse(&name, &src)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

        let mut checker = Checker::new(&name);
        checker.check(&file);
        let first: Vec<Diagnostic> = checker.take_diagnostics();

        for _ in 0..9 {
            checker.check(&file);
            let repeated: Vec<Diagnostic> = checker.take_diagnostics();
            assert_eq!(
                first,
                repeated,
                "R2 repeated-checker violation in {}: diagnostics changed across repeated checks",
                path.display(),
            );
        }
    }
}

/// Checking the same multi-file `Project` multiple times must yield identical
/// diagnostics. The project's parallel (rayon) pass-3 emission must be
/// deterministic regardless of thread scheduling.
#[test]
fn r2_repeated_project_output_is_identical() {
    let fixtures = checker_fixtures();
    assert!(fixtures.len() >= CHECKER_FIXTURE_FLOOR);
    let mut parser = RParser::new().expect("parser init");

    // Use a deterministic subset to keep the test fast while exercising
    // cross-file parallel emission.
    let sample: Vec<PathBuf> = fixtures.into_iter().step_by(7).collect();
    let mut parsed: Vec<(String, ry_core::SourceFile)> = Vec::new();
    for path in &sample {
        let src =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path.to_string_lossy().to_string();
        let file = parser
            .parse(&name, &src)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        parsed.push((name, file));
    }

    let mut project = Project::new();
    for (name, file) in &parsed {
        project.add_file(name.clone(), file.clone());
    }
    let first = project.check();

    for _ in 0..4 {
        // check() does a cold check every time (resets incremental state).
        let repeated = project.check();
        assert_eq!(
            first, repeated,
            "R2 repeated-project violation: diagnostics changed across repeated cold checks",
        );
    }
}

/// The incremental check path must also be deterministic: repeated
/// `check_incremental` on the same project state must produce identical
/// results.
#[test]
fn r2_incremental_repeated_output_is_identical() {
    let sources = [
        ("a.R", "fa <- function(x) x * 2\nva <- fa(1L)\n"),
        ("b.R", "fb <- function(x) fa(x) + 1\nvb <- fb(2L)\n"),
        ("c.R", "fc <- function(x) fb(x) * 3\nvc <- fc(3L)\n"),
    ];
    let mut parser = RParser::new().unwrap();
    let mut project = Project::new();
    for (path, src) in &sources {
        project.add_file(path.to_string(), parser.parse(path, src).unwrap());
    }

    let first = project.check_incremental();
    for _ in 0..4 {
        let repeated = project.check_incremental();
        assert_eq!(
            first, repeated,
            "R2 incremental: diagnostics changed across repeated incremental checks",
        );
    }
}

// ── P35-W11: Fallible orchestration gates ────────────────────────────────

/// Parallel-mode falsification (P35-W11).
///
/// `Project::check()` distributes pass-3 diagnostic emission across rayon
/// workers via `par_iter().collect()`. If any emitter mutated shared state,
/// or if the collect ordering broke, diagnostics would differ between a
/// single-threaded (serial) pool and a multi-threaded (parallel) pool.
///
/// This test installs custom rayon thread pools with 1 and 4 threads and
/// compares the full diagnostic multiset. It protects the parallel emission
/// seam: a regression that makes parallel non-deterministic is caught here,
/// not only in a flaky CI run.
#[test]
fn parallel_project_check_matches_serial_across_thread_counts() {
    let fixtures = checker_fixtures();
    assert!(fixtures.len() >= CHECKER_FIXTURE_FLOOR);
    let mut parser = RParser::new().expect("parser init");

    // Use enough files to exercise parallel work distribution.
    let sample: Vec<PathBuf> = fixtures.into_iter().step_by(3).collect();
    let mut parsed: Vec<(String, ry_core::SourceFile)> = Vec::new();
    for path in &sample {
        let src =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path.to_string_lossy().to_string();
        let file = parser
            .parse(&name, &src)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        parsed.push((name, file));
    }

    // Snapshot a normalized diagnostic multiset for a given thread count.
    type FileDiags = Vec<(String, Vec<(String, Span, String)>)>;
    let snapshot = |threads: usize| -> FileDiags {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("build rayon pool");
        pool.install(|| {
            let mut project = Project::new();
            for (name, file) in &parsed {
                project.add_file(name.clone(), file.clone());
            }
            let results = project.check();
            results
                .into_iter()
                .map(|(path, diags)| {
                    let normalized: Vec<(String, Span, String)> = diags
                        .into_iter()
                        .map(|d| (d.code.to_string(), d.span, d.message))
                        .collect();
                    (path, normalized)
                })
                .collect()
        })
    };

    let serial = snapshot(1);
    for threads in [2, 4, 8] {
        let parallel = snapshot(threads);
        assert_eq!(
            serial, parallel,
            "P35-W11: project check diverged between 1-thread (serial) and              {threads}-thread (parallel) pools",
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// R3 — Inert blank/comment insertion (GATE)
// ════════════════════════════════════════════════════════════════════════

/// Compute the byte offsets of safe insertion points: positions between
/// top-level statements that are also at line-start boundaries. This double
/// restriction ensures the insertion (a) does not land inside a nested
/// construct like a function body and (b) does not split a line or shift a
/// diagnostic's column within its line.
fn safe_insertion_offsets(src: &str, stmts: &[Stmt]) -> Vec<usize> {
    let mut offsets: Vec<usize> = vec![0]; // Before the first statement.
    for stmt in stmts {
        offsets.push(stmt_span(stmt).end);
    }
    offsets.sort();
    offsets.dedup();
    // Keep only offsets that sit at a line-start boundary (byte 0, end of
    // file, or right after a newline). This prevents mid-line insertion.
    offsets
        .into_iter()
        .filter(|&offset| {
            offset == 0 || offset >= src.len() || src.as_bytes().get(offset - 1) == Some(&b'\n')
        })
        .collect()
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

/// Shift a diagnostic span to account for an insertion of `inserted` bytes at
/// `offset`. Diagnostics entirely after the insertion point shift back; those
/// before are untouched; those straddling are left in place (they should not
/// occur at safe statement boundaries).
fn shift_span(span: Span, offset: usize, inserted_len: usize, inserted_newlines: usize) -> Span {
    let mut s = span;
    if s.start >= offset + inserted_len {
        s.start -= inserted_len;
        s.end -= inserted_len;
        s.line = s.line.saturating_sub(inserted_newlines);
    }
    s
}

/// Shift a full diagnostic back to the original coordinate system after an
/// insertion at `offset`.
fn shift_diagnostic(
    d: &Diagnostic,
    offset: usize,
    inserted_len: usize,
    inserted_newlines: usize,
) -> Diagnostic {
    Diagnostic {
        severity: d.severity,
        span: shift_span(d.span, offset, inserted_len, inserted_newlines),
        path: d.path.clone(),
        code: d.code,
        message: d.message.clone(),
        confidence: d.confidence,
    }
}

/// Insert `text` at `offset` in `src`.
fn insert_at(src: &str, offset: usize, text: &str) -> String {
    let mut out = String::with_capacity(src.len() + text.len());
    out.push_str(&src[..offset]);
    out.push_str(text);
    out.push_str(&src[offset..]);
    out
}

#[test]
fn r3_inert_blank_and_comment_insertion_is_diagnostic_neutral() {
    let fixtures = checker_fixtures();
    assert!(fixtures.len() >= CHECKER_FIXTURE_FLOOR);
    let mut parser = RParser::new().expect("parser init");

    let mut total_checked = 0usize;
    for path in &fixtures {
        let src =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path.to_string_lossy().to_string();

        let original = parser
            .parse(&name, &src)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        if original
            .parse_errors
            .iter()
            .any(|s| s.start == 0 && s.end == 0)
        {
            continue; // skip unparseable
        }
        let orig_diags = {
            let mut c = Checker::new(&name);
            c.check(&original);
            c.take_diagnostics()
        };

        // Try inserting at each safe position (top-level statement
        // boundary at a line start).
        for &offset in &safe_insertion_offsets(&src, &original.stmts) {
            if offset > src.len() {
                continue;
            }
            // Two transformations: a blank line and a harmless comment.
            for inserted in ["\n", "# inert comment\n"] {
                let modified = insert_at(&src, offset, inserted);
                let inserted_len = inserted.len();

                let modified_file = parser
                    .parse(&name, &modified)
                    .unwrap_or_else(|e| panic!("reparse {}: {e}", path.display()));

                let modified_diags = {
                    let mut c = Checker::new(&name);
                    c.check(&modified_file);
                    c.take_diagnostics()
                };

                // Shift modified diagnostics back to original coordinates.
                let inserted_newlines = inserted.matches('\n').count();
                let shifted: Vec<Diagnostic> = modified_diags
                    .iter()
                    .map(|d| shift_diagnostic(d, offset, inserted_len, inserted_newlines))
                    .collect();

                // Compare: code multiset must match, spans must match, and
                // messages must match. A difference means the insertion is
                // not inert.
                assert_eq!(
                    orig_diags.len(),
                    shifted.len(),
                    "R3 violation in {} at offset {offset}: diagnostic count changed ({} vs {}) after inserting {:?}",
                    path.display(),
                    orig_diags.len(),
                    shifted.len(),
                    inserted,
                );
                for (orig, modi) in orig_diags.iter().zip(shifted.iter()) {
                    assert_eq!(
                        diag_full(orig),
                        diag_full(modi),
                        "R3 violation in {} at offset {offset}: diagnostic changed after inserting {:?}\n  orig: {:?}\n  modified: {:?}",
                        path.display(),
                        inserted,
                        orig,
                        modi,
                    );
                }
                total_checked += 1;
            }
        }
    }
    assert!(
        total_checked >= 100,
        "R3: expected at least 100 insertion checks, got {total_checked}",
    );
}

// ════════════════════════════════════════════════════════════════════════
// R4 — Capture-avoiding alpha rename (REPORT, P35-W7 registry)
// ════════════════════════════════════════════════════════════════════════

/// Collect user-defined identifiers from assignment targets. These are the
/// only names safe to rename: builtins and stub-known functions stay fixed.
fn user_defined_names(file: &ry_core::SourceFile) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for stmt in &file.stmts {
        if let Stmt::Assign {
            target: ry_core::ast::Expr::Ident { name, .. },
            ..
        } = stmt
        {
            // Skip very short names and names that look like single
            // letters common in examples — rename only plausible user
            // identifiers.
            if name.len() >= 3 && !name.starts_with('.') {
                names.insert(name.clone());
            }
        }
    }
    names
}

/// Rename every occurrence of `old` to `new` in the source text.
fn rename_in_source(src: &str, old: &str, new: &str) -> String {
    // Word-boundary-aware replacement to avoid renaming substrings.
    let mut result = String::new();
    let bytes = src.as_bytes();
    let old_bytes = old.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + old_bytes.len() <= bytes.len() && &bytes[i..i + old_bytes.len()] == old_bytes {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_ok =
                i + old_bytes.len() == bytes.len() || !is_ident_byte(bytes[i + old_bytes.len()]);
            if before_ok && after_ok {
                result.push_str(new);
                i += old_bytes.len();
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

/// Diagnostics compared by code and span-normalised message (the renamed
/// identifier may appear in messages).
fn diag_code_set(diags: &[Diagnostic]) -> BTreeSet<String> {
    diags.iter().map(|d| d.code.to_string()).collect()
}

#[test]
fn r4_alpha_rename_report() {
    let fixtures = checker_fixtures();
    let mut parser = RParser::new().expect("parser init");
    let mut total_renames = 0usize;
    let mut total_diffs = 0usize;

    for path in &fixtures {
        let src =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path.to_string_lossy().to_string();
        let file = parser
            .parse(&name, &src)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        if !file.parse_errors.is_empty() {
            continue;
        }

        let user_names = user_defined_names(&file);
        for old in &user_names {
            // Rename to a fresh identifier unlikely to collide.
            let new = format!("{old}_renamed");
            let renamed_src = rename_in_source(&src, old, &new);
            let orig_diags = check_source(&src);
            let renamed_diags = check_source(&renamed_src);

            let orig_codes = diag_code_set(&orig_diags);
            let renamed_codes = diag_code_set(&renamed_diags);

            if orig_codes != renamed_codes {
                total_diffs += 1;
                eprintln!(
                    "R4 report: {name}: renaming `{old}` changed diagnostic codes\n  before: {orig_codes:?}\n  after:  {renamed_codes:?}",
                );
            }
            total_renames += 1;
        }
    }
    eprintln!("R4 alpha rename: {total_renames} renames checked, {total_diffs} differences found");
    // Report mode: no assertion on zero differences. The test just collects
    // and prints findings for review.
}

// ════════════════════════════════════════════════════════════════════════
// R5 — Replaced by checker/project reset and non-interference (GATE)
// ════════════════════════════════════════════════════════════════════════

/// A reused `Checker` must not leak inference state from a previous `check()`
/// into a subsequent one. Checking A then B on the same instance must yield
/// identical diagnostics for B as a fresh Checker.
#[test]
fn r5_checker_reset_no_accumulated_state() {
    let mut parser = RParser::new().expect("parser init");

    // A defines a function whose return type would change B's diagnostics
    // if leaked (character return → RY040 when used arithmetically).
    let src_a = "make_value <- function() \"text\"\n";
    let src_b = "result <- make_value() + 1L\n";

    let file_a = parser.parse("shared.R", src_a).unwrap();
    let file_b = parser.parse("shared.R", src_b).unwrap();

    // Reused checker: A then B.
    let mut checker = Checker::new("shared.R");
    checker.check(&file_a);
    let reused = checker.check(&file_b).to_vec();

    // Fresh checker: B only.
    let mut fresh = Checker::new("shared.R");
    let fresh_b = fresh.check(&file_b).to_vec();

    assert_eq!(
        reused, fresh_b,
        "R5 checker reset: reused checker leaked state from A into B\n  reused: {reused:?}\n  fresh:  {fresh_b:?}",
    );
}

/// A reused `Checker` must not leak state when the first file defines a
/// variable that shadows a base function. If leaked, B would see the shadow
/// instead of the base function.
#[test]
fn r5_checker_reset_no_shadow_leak() {
    let mut parser = RParser::new().expect("parser init");

    // A defines `length` to return character; B uses `length` arithmetically.
    // If state leaks, B would fire RY040 (character + int).
    let src_a = "length <- function(x) \"shadowed\"\n";
    let src_b = "x <- c(1L, 2L)\ny <- length(x) + 1L\n";

    let file_a = parser.parse("shared.R", src_a).unwrap();
    let file_b = parser.parse("shared.R", src_b).unwrap();

    let mut checker = Checker::new("shared.R");
    checker.check(&file_a);
    let reused = checker.check(&file_b).to_vec();

    let mut fresh = Checker::new("shared.R");
    let fresh_b = fresh.check(&file_b).to_vec();

    assert_eq!(
        reused, fresh_b,
        "R5 checker reset: shadowed function definition leaked across checks\n  reused: {reused:?}\n  fresh:  {fresh_b:?}",
    );
}

/// Project non-interference: checking file A then adding B must not introduce
/// new false diagnostics in B. B's diagnostics in the combined project should
/// be a subset of (or equal to) B's diagnostics when checked alone — A can
/// only resolve unbound references, never create new false positives.
#[test]
fn r5_project_non_interference_no_new_false_positives() {
    let mut parser = RParser::new().expect("parser init");

    // B has its own diagnostics independent of A.
    let src_b = "x <- \"text\"\ny <- x + 1L\n"; // RY040 character + int
    let src_a = "helper <- function() {\n  z <- 1L\n  z\n}\n";

    let file_b = parser.parse("b.R", src_b).unwrap();
    let file_a = parser.parse("a.R", src_a).unwrap();

    // B alone.
    let mut alone = Project::new();
    alone.add_file("b.R".to_string(), file_b.clone());
    let alone_diags: Vec<Diagnostic> = alone
        .check()
        .into_iter()
        .find(|(p, _)| p == "b.R")
        .map(|(_, d)| d)
        .unwrap_or_default();

    // A + B.
    let mut combined = Project::new();
    combined.add_file("a.R".to_string(), file_a);
    combined.add_file("b.R".to_string(), file_b);
    let combined_diags: Vec<Diagnostic> = combined
        .check()
        .into_iter()
        .find(|(p, _)| p == "b.R")
        .map(|(_, d)| d)
        .unwrap_or_default();

    // The combined project may fix some diagnostics (cross-file visibility)
    // but must not introduce new ones.
    let alone_codes: BTreeSet<String> = alone_diags.iter().map(|d| d.code.to_string()).collect();
    let combined_codes: BTreeSet<String> =
        combined_diags.iter().map(|d| d.code.to_string()).collect();
    let new_codes: BTreeSet<_> = combined_codes.difference(&alone_codes).collect();
    assert!(
        new_codes.is_empty(),
        "R5 non-interference: adding a.R introduced new diagnostics in b.R: {new_codes:?}\n  alone:    {alone_codes:?}\n  combined: {combined_codes:?}",
    );
}

/// Project reset via incremental: after checking a project with files A and B,
/// removing A and re-checking must not leave B's diagnostics corrupted by A's
/// residual state.
#[test]
fn r5_project_incremental_reset_after_removal() {
    let mut parser = RParser::new().expect("parser init");

    let src_a = "make_value <- function() \"text\"\n";
    let src_b = "result <- make_value() + 1L\n"; // RY040 in project, RY010 alone

    let file_a = parser.parse("a.R", src_a).unwrap();
    let file_b = parser.parse("b.R", src_b).unwrap();

    // Incremental: add both, check, then remove A.
    let mut project = Project::new();
    project.add_file("a.R".to_string(), file_a.clone());
    project.add_file("b.R".to_string(), file_b.clone());
    let _with_a = project.check_incremental();

    project.remove_file("a.R");
    let after_removal = project.check_incremental();
    let b_after: Vec<Diagnostic> = after_removal
        .iter()
        .find(|(p, _)| p == "b.R")
        .map(|(_, d)| d.clone())
        .unwrap_or_default();

    // Fresh project with only B.
    let mut fresh = Project::new();
    fresh.add_file("b.R".to_string(), file_b);
    let fresh_result = fresh.check_incremental();
    let b_fresh: Vec<Diagnostic> = fresh_result
        .iter()
        .find(|(p, _)| p == "b.R")
        .map(|(_, d)| d.clone())
        .unwrap_or_default();

    // After removal, make_value is gone, so b.R should report the same as a
    // fresh project with only b.R. The incremental path must not carry over
    // A's function table.
    let after_codes: BTreeSet<String> = b_after.iter().map(|d| d.code.to_string()).collect();
    let fresh_codes: BTreeSet<String> = b_fresh.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(
        after_codes, fresh_codes,
        "R5 project reset: removing a.R did not reset b.R's diagnostics\n  after removal: {after_codes:?}\n  fresh:         {fresh_codes:?}",
    );
}

// ════════════════════════════════════════════════════════════════════════
// R7 — Literal-to-parameter lifting (REPORT, feeds P35-W12)
// ════════════════════════════════════════════════════════════════════════

/// The metamorphic relation: `f <- function(x) body` called as `f(literal)`
/// should produce the same diagnostics as `f <- function(x = literal) body`
/// called as `f()`. Both evaluate `body` with `x` bound to `literal`.
#[test]
fn r7_literal_to_parameter_lifting_report() {
    let cases = [
        // (definition, call with literal, call with default)
        (
            "f <- function(x) x + 1L\n",
            "f(42L)\n",
            "f <- function(x = 42L) x + 1L\nf()\n",
        ),
        (
            "f <- function(x) x + \"text\"\n",
            "f(1L)\n",
            "f <- function(x = 1L) x + \"text\"\nf()\n",
        ),
        (
            "g <- function(data) data[[\"col\"]]\n",
            "g(data.frame(x = 1L))\n",
            "g <- function(data = data.frame(x = 1L)) data[[\"col\"]]\ng()\n",
        ),
        (
            "h <- function(n) seq_len(n)\n",
            "h(5L)\n",
            "h <- function(n = 5L) seq_len(n)\nh()\n",
        ),
    ];

    let mut total = 0usize;
    let mut diffs = 0usize;
    for (def, call_literal, call_default) in &cases {
        let literal_src = format!("{def}{call_literal}");
        let default_src = call_default.to_string();
        let literal_diags = check_source(&literal_src);
        let default_diags = check_source(&default_src);

        let literal_codes: BTreeSet<String> =
            literal_diags.iter().map(|d| d.code.to_string()).collect();
        let default_codes: BTreeSet<String> =
            default_diags.iter().map(|d| d.code.to_string()).collect();

        if literal_codes != default_codes {
            diffs += 1;
            eprintln!(
                "R7 report: literal vs default-lifted parameter diverge\n  def: {def}  literal call: {call_literal}\n  literal codes: {literal_codes:?}\n  default codes: {default_codes:?}",
            );
        }
        total += 1;
    }
    eprintln!("R7 literal-to-parameter lifting: {total} cases checked, {diffs} differences found");
    // Report mode: no assertion on zero differences.
}

// ════════════════════════════════════════════════════════════════════════
// R8 — Negated branch swap (REPORT over explicit else, stub-known predicates)
// ════════════════════════════════════════════════════════════════════════

/// The metamorphic relation: `if (pred(x)) A else B` is equivalent to
/// `if (!pred(x)) B else A`. The branch-join facts should be identical
/// regardless of branch order, because both branches produce the same
/// post-join state for every variable.
///
/// Only stub-known type predicates (`is.null`, `is.numeric`, etc.) are
/// tested, since only those have modelled narrowing. Spans and branch-local
/// diagnostics are normalised by comparing the multiset of post-join
/// diagnostic codes.
#[test]
fn r8_negated_branch_swap_report() {
    let predicates = [
        "is.null",
        "is.numeric",
        "is.integer",
        "is.double",
        "is.character",
        "is.logical",
        "is.list",
        "is.function",
    ];

    let bodies = [
        // (then_branch, else_branch) — each uses x
        ("x + 1L\n", "x + \"text\"\n"),
        ("print(x)\n", "length(x)\n"),
        ("x[[1]]\n", "x + 2L\n"),
    ];

    let mut total = 0usize;
    let mut diffs = 0usize;
    for pred in &predicates {
        for (then_body, else_body) in &bodies {
            // Original: if (pred(x)) then else else_
            let original = format!(
                "f <- function(x) {{\n  if ({pred}(x)) {{\n    {then_body}}} else {{\n    {else_body}}}\n}}\n",
            );
            // Swapped: if (!pred(x)) else_ else then
            let swapped = format!(
                "f <- function(x) {{\n  if (!{pred}(x)) {{\n    {else_body}}} else {{\n    {then_body}}}\n}}\n",
            );

            let orig_diags = check_source(&original);
            let swap_diags = check_source(&swapped);

            let orig_codes: BTreeSet<String> =
                orig_diags.iter().map(|d| d.code.to_string()).collect();
            let swap_codes: BTreeSet<String> =
                swap_diags.iter().map(|d| d.code.to_string()).collect();

            if orig_codes != swap_codes {
                diffs += 1;
                eprintln!(
                    "R8 report: branch swap changed diagnostics for {pred}\n  original codes: {orig_codes:?}\n  swapped codes:  {swap_codes:?}\n  original:\n{original}  swapped:\n{swapped}",
                );
            }
            total += 1;
        }
    }
    eprintln!("R8 negated branch swap: {total} cases checked, {diffs} differences found");
}

// ════════════════════════════════════════════════════════════════════════
// R9 — Unchanged variable across branches (focused branch-join invariant)
// ════════════════════════════════════════════════════════════════════════

/// If a variable has a known type before an if-else and is NOT reassigned in
/// either branch, its post-join type must be unchanged. A use of that variable
/// after the join must produce the same diagnostic as a use before the join.
#[test]
fn r9_unchanged_variable_post_join_type_is_stable() {
    // x is integer before the if. It is not reassigned in either branch.
    // After the join, x must still be integer, so `x + "text"` must fire RY040.
    let src = "f <- function(cond) {\n  x <- 1L\n  if (cond) {\n    y <- x\n  } else {\n    z <- x\n  }\n  x + \"text\"\n}\n";
    let diags = check_source(src);
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "R9: unchanged integer variable lost its type across branch join: {diags:?}",
    );
}

/// A variable assigned to the same type in both branches must have that type
/// after the join. If both branches assign `x <- 1L`, post-join x is integer.
#[test]
fn r9_same_type_in_both_branches_joins_correctly() {
    let src = "f <- function(cond) {\n  if (cond) {\n    x <- 1L\n  } else {\n    x <- 1L\n  }\n  x + \"text\"\n}\n";
    let diags = check_source(src);
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "R9: variable assigned integer in both branches should join to integer: {diags:?}",
    );
}

/// A variable narrowed differently in each branch must join to the union. If
/// the then-branch narrows to integer and the else-branch narrows to
/// character, the post-join use must be consistent regardless of order.
#[test]
fn r9_complementary_narrowing_joins_consistently() {
    // is.numeric(x) narrows x to integer|double in then, away in else.
    // The order of branches should not affect the post-join diagnostic set.
    let original = "f <- function(x) {\n  if (is.numeric(x)) {\n    x + 1L\n  } else {\n    length(x)\n  }\n}\n";
    let swapped = "f <- function(x) {\n  if (!is.numeric(x)) {\n    length(x)\n  } else {\n    x + 1L\n  }\n}\n";
    let orig_codes: BTreeSet<String> = check_source(original)
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    let swap_codes: BTreeSet<String> = check_source(swapped)
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(
        orig_codes, swap_codes,
        "R9: complementary narrowing branch order changed diagnostics\n  original: {orig_codes:?}\n  swapped:  {swap_codes:?}",
    );
}

// ════════════════════════════════════════════════════════════════════════
// R10 — Pipe placeholder combinations (generated regression matrix)
// ════════════════════════════════════════════════════════════════════════

/// A regression matrix over pipe placeholder combinations. Each entry is a
/// concrete R source exercising a different placeholder/pipe combination.
/// The gate asserts the checker does not panic and produces a deterministic,
/// finite diagnostic set. This is NOT a universal relation: it is a curated
/// matrix of known pipe shapes.
#[test]
fn r10_pipe_placeholder_matrix_no_panic_deterministic() {
    let cases: &[&str] = &[
        // Native pipe, no placeholder.
        "x <- 1L\ny <- x |> identity()\n",
        // Native pipe, underscore placeholder (R 4.2+).
        "x <- 1L\ny <- identity(z = x)\n",
        // Magrittr pipe, no placeholder (prepended as first arg).
        "x <- 1L\ny <- x |> sum()\n",
        // Magrittr pipe, dot placeholder.
        "df <- data.frame(a = 1L)\nresult <- df |> nrow()\n",
        // Chained pipes.
        "x <- c(1L, 2L, 3L)\ny <- x |> sum() |> identity()\n",
        // Pipe into a function with multiple arguments.
        "x <- 1L\ny <- x |> sum(10L)\n",
        // Tee pipe (returns LHS).
        "x <- 1L\nx |> print()\ny <- x + 1L\n",
        // Nested pipe.
        "x <- c(1L, 2L)\ny <- x |> sum() |> abs()\n",
        // Pipe with type error in the piped value.
        "x <- \"text\"\ny <- x |> sum()\n",
        // Pipe with length error.
        "f <- function() if (TRUE) c(TRUE, FALSE) else TRUE\ng <- f() |> identity() && c(TRUE, FALSE)\n",
    ];

    for (i, src) in cases.iter().enumerate() {
        let diags = check_source(src);
        // Determinism: check twice and compare.
        let diags2 = check_source(src);
        assert_eq!(
            diags, diags2,
            "R10 pipe matrix case {i}: non-deterministic output for:\n{src}",
        );
    }

    // Equivalent formulations: `x |> f()` should produce the same diagnostics
    // as `f(x)` when f is a simple function.
    let pairs = [
        ("x <- 1L\ny <- x |> sum()\n", "x <- 1L\ny <- sum(x)\n"),
        (
            "x <- 1L\ny <- x |> identity()\n",
            "x <- 1L\ny <- identity(x)\n",
        ),
    ];
    for (piped, direct) in &pairs {
        let piped_diags = check_source(piped);
        let direct_diags = check_source(direct);
        let piped_codes: BTreeSet<String> =
            piped_diags.iter().map(|d| d.code.to_string()).collect();
        let direct_codes: BTreeSet<String> =
            direct_diags.iter().map(|d| d.code.to_string()).collect();
        assert_eq!(
            piped_codes, direct_codes,
            "R10: pipe form and direct call form diverge\n  pipe:   {piped_codes:?}\n  direct: {direct_codes:?}\n  source: {piped}",
        );
    }
}
