//! Corpus harness.
//!
//! Walks `testdata/*.R`, parses the `# expect:` / `# no-diag` marker from
//! the leading comment, runs the checker, and asserts that the multiset of
//! emitted diagnostic codes matches the fixture's expectation *exactly*.
//!
//! This catches both regressions (a fixture that used to fire stops firing)
//! and false-positive leaks (a fixture that should be silent starts firing).
//!
//! `testdata/cases/<topic>.R` holds many small cases per file. Each case is
//! a `## case: <name>` line, a marker line, and R source, checked alone.

use std::collections::BTreeMap;
use std::fs;

use ry_checker::{Checker, Severity};
use ry_core::RParser;

#[derive(Debug)]
enum Expectation {
    /// Exactly these codes, no more, no less. Order-independent.
    Codes(Vec<String>),
    /// No diagnostics at all.
    None,
    /// At least one parse/syntax error (`RY000`) must be emitted.
    ParseError,
}

#[derive(Debug)]
struct Fixture {
    name: String,
    src: String,
    expected: Expectation,
}

fn parse_marker(src: &str) -> Option<Expectation> {
    // Only inspect the very first line for the marker; this keeps the
    // grammar unambiguous and avoids accidental matches in code samples.
    let first = src.lines().next()?;
    // A BOM-prefixed fixture (err_bom.R) carries its BOM before the
    // marker; skip it so the marker still reads as the first line.
    let first = first.strip_prefix('\u{feff}').unwrap_or(first);
    let trimmed = first.trim_start_matches([' ', '\t']);
    if !trimmed.starts_with('#') {
        return None;
    }
    let body = trimmed.trim_start_matches('#').trim();
    if body.is_empty() {
        return None;
    }
    // `# no-diag` is a standalone marker; `# expect: ...` is colon-delimited.
    if body.eq_ignore_ascii_case("no-diag") || body.eq_ignore_ascii_case("no_diag") {
        return Some(Expectation::None);
    }
    if body.eq_ignore_ascii_case("expect-parse-error")
        || body.eq_ignore_ascii_case("expect_parse_error")
    {
        return Some(Expectation::ParseError);
    }
    let (key, value) = body.split_once(':')?;
    let key = key.trim();
    let value = value.trim();
    if key.eq_ignore_ascii_case("expect") {
        let codes: Vec<String> = value
            .split([',', ' '])
            .filter(|s| !s.is_empty())
            .map(|s| s.trim().to_string())
            .collect();
        Some(Expectation::Codes(codes))
    } else {
        None
    }
}

fn load_fixtures() -> Vec<Fixture> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let mut out = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => panic!("testdata dir {:?}: {}", dir, e),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("R") {
            continue;
        }
        let src =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
        let expected = parse_marker(&src).unwrap_or_else(|| {
            panic!(
                "fixture {} has no `# expect:` or `# no-diag` marker on its first line",
                path.display()
            )
        });
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        out.push(Fixture {
            name,
            src,
            expected,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn run_diagnostics(name: &str, src: &str) -> Vec<ry_checker::Diagnostic> {
    let mut parser = RParser::new().expect("parser init");
    let mut file = parser
        .parse(name, src)
        .unwrap_or_else(|e| panic!("parse {}: {}", name, e));
    // The harness reads fixtures as text, so it applies the on-disk read
    // boundary's leading-BOM rule itself (#474), the same way the CLI
    // pipeline and the LSP index attach the boundary's findings.
    if src.starts_with('\u{feff}') {
        file.leading_bom = true;
    }
    let mut c = Checker::new(name);
    c.check(&file);
    // Apply inline suppression (`# ry: ignore`, `# noqa`,
    // `# ry: ignore-file`) so corpus fixtures that test the suppression
    // feature behave the same way the CLI / LSP do. Use the lexical
    // (comment-based) filter so a `#` inside a string literal is not
    // mistaken for a suppression directive.
    ry_checker::filter_suppressed_with_comments(c.take_diagnostics(), &file.comments, src)
}

fn code_counts<'a>(codes: impl IntoIterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut counts = BTreeMap::new();
    for code in codes {
        *counts.entry(code).or_insert(0) += 1;
    }
    counts
}

fn expectation_holds(expected: &Expectation, got: &BTreeMap<&str, usize>) -> bool {
    match expected {
        Expectation::None => got.is_empty(),
        Expectation::ParseError => got.contains_key("RY000"),
        Expectation::Codes(codes) => code_counts(codes.iter().map(String::as_str)) == *got,
    }
}

fn run(name: &str, src: &str) -> Vec<(String, Severity)> {
    run_diagnostics(name, src)
        .into_iter()
        .map(|d| (d.code.to_string(), d.severity))
        .collect()
}

#[test]
fn corpus_has_fixtures() {
    let f = load_fixtures();
    assert!(
        !f.is_empty(),
        "no fixtures found in testdata/ - did the directory get removed?"
    );
}

#[test]
fn corpus_all_fixtures_have_markers() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("R") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        assert!(
            parse_marker(&src).is_some(),
            "{}: missing `# expect:` or `# no-diag` marker on first line",
            path.display()
        );
    }
}

/// The actual per-fixture assertions. Each fixture becomes a separate
/// test case, so failures name the offending file rather than collapsing
/// into one big panic.
#[test]
fn corpus_check_each_fixture() {
    let fixtures = load_fixtures();
    let mut failures: Vec<String> = Vec::new();
    let mut total: usize = 0;
    let mut passing: usize = 0;

    for fx in &fixtures {
        total += 1;
        let got = run(&fx.name, &fx.src);
        let got_codes = code_counts(got.iter().map(|(code, _)| code.as_str()));
        if expectation_holds(&fx.expected, &got_codes) {
            passing += 1;
        } else {
            failures.push(format!(
                "{}: expected {:?}, got {:?}",
                fx.name, fx.expected, got_codes
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "corpus: {}/{} fixtures passed, {} failed:\n  - {}\n",
            passing,
            total,
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

/// Reviewable message gate for every checker fixture. This deliberately
/// stores normalized fields rather than a digest or the ignored ecosystem
/// `.full.txt` reports, so wording drift appears as a normal snapshot diff.
#[test]
fn checker_fixture_diagnostics_are_readable_snapshots() {
    let snapshots = load_fixtures()
        .into_iter()
        .map(|fixture| {
            let diagnostics = run_diagnostics(&fixture.name, &fixture.src)
                .into_iter()
                .map(|diagnostic| {
                    serde_json::json!({
                        "code": diagnostic.code,
                        "severity": diagnostic.severity.as_str(),
                        "span": {
                            "start": diagnostic.span.start,
                            "end": diagnostic.span.end,
                            "line": diagnostic.span.line,
                            "column": diagnostic.span.col,
                        },
                        "message": diagnostic.message,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({"fixture": fixture.name, "diagnostics": diagnostics})
        })
        .collect::<Vec<_>>();
    insta::assert_yaml_snapshot!("checker_fixture_diagnostics", snapshots);
}

/// For visibility: list every fixture and what it currently emits. This
/// is a no-op assertion that exists purely so `cargo test corpus_summary
/// -- --nocapture` gives a quick status table while iterating.
#[test]
fn corpus_summary() {
    let fixtures = load_fixtures();
    println!("{:<32} {:<10} codes", "fixture", "result");
    println!("{}", "-".repeat(70));
    for fx in &fixtures {
        let got = run(&fx.name, &fx.src);
        let codes: Vec<&str> = got.iter().map(|(c, _)| c.as_str()).collect();
        let expected_str = match &fx.expected {
            Expectation::None => "(none)".to_string(),
            Expectation::ParseError => "(parse-error)".to_string(),
            Expectation::Codes(c) => c.join(","),
        };
        let result = if codes.is_empty() {
            "(none)".to_string()
        } else {
            codes.join(",")
        };
        println!("{:<32} {:<10} {}", fx.name, expected_str, result);
    }
}

/// One `## case:` block of a `testdata/cases/<topic>.R` file.
struct Case {
    name: String,
    /// 1-based file line of the first source line after the marker.
    first_line: usize,
    src: String,
    expected: Expectation,
}

/// Split a case file into cases. A case is a `## case: <name>` line, a
/// marker line in the fixture grammar, and the R source up to the next
/// `## case:` line.
fn load_cases(path: &std::path::Path) -> Vec<Case> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let lines: Vec<&str> = text.lines().collect();
    let mut cases = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(name) = lines[i].strip_prefix("## case:") else {
            assert!(
                lines[i].is_empty() || lines[i].starts_with('#'),
                "{}:{}: code before the first `## case:` line",
                path.display(),
                i + 1
            );
            i += 1;
            continue;
        };
        let name = name.trim().to_string();
        let expected = lines
            .get(i + 1)
            .and_then(|line| parse_marker(line))
            .unwrap_or_else(|| panic!("{}: case `{name}` has no marker line", path.display()));
        let start = i + 2;
        let mut end = start;
        while end < lines.len() && !lines[end].starts_with("## case:") {
            end += 1;
        }
        i = end;
        while end > start && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        cases.push(Case {
            name,
            first_line: start + 1,
            src: lines[start..end].join("\n") + "\n",
            expected,
        });
    }
    cases
}

/// Checks every case in `testdata/cases/*.R` on its own, like a fixture
/// file, and snapshots each file's diagnostics so a diff names the case.
#[test]
fn case_files_match_their_expectations() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/cases");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("R"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no case files in {}", dir.display());
    let mut failures = Vec::new();
    let mut snapshots = Vec::new();
    for path in &paths {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut rows = Vec::new();
        for case in load_cases(path) {
            let diagnostics = run_diagnostics("test.R", &case.src);
            let got = code_counts(diagnostics.iter().map(|d| d.code));
            if !expectation_holds(&case.expected, &got) {
                failures.push(format!(
                    "{file}: case `{}`: expected {:?}, got {got:?}",
                    case.name, case.expected
                ));
            }
            if !diagnostics.is_empty() {
                let diagnostics = diagnostics
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "code": d.code,
                            "severity": d.severity.as_str(),
                            "line": case.first_line + d.span.line,
                            "message": d.message,
                        })
                    })
                    .collect::<Vec<_>>();
                rows.push(serde_json::json!({"case": case.name, "diagnostics": diagnostics}));
            }
        }
        snapshots.push((file.trim_end_matches(".R").to_string(), rows));
    }
    assert!(
        failures.is_empty(),
        "{} case(s) failed:\n  - {}",
        failures.len(),
        failures.join("\n  - ")
    );
    for (topic, rows) in snapshots {
        insta::assert_yaml_snapshot!(format!("cases_{topic}"), rows);
    }
}
