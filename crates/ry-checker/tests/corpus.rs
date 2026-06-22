//! Corpus harness.
//!
//! Walks `testdata/*.R`, parses the `# expect:` / `# no-diag` marker from
//! the leading comment, runs the checker, and asserts that the multiset of
//! emitted diagnostic codes matches the fixture's expectation *exactly*.
//!
//! This catches both regressions (a fixture that used to fire stops firing)
//! and false-positive leaks (a fixture that should be silent starts firing).

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
        let src = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
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
        out.push(Fixture { name, src, expected });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn run(name: &str, src: &str) -> Vec<(String, Severity)> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse(name, src)
        .unwrap_or_else(|e| panic!("parse {}: {}", name, e));
    let mut c = Checker::new(name);
    c.check(&file);
    // Apply inline suppression (`# ry: ignore`, `# noqa`,
    // `# ry: ignore-file`) so corpus fixtures that test the suppression
    // feature behave the same way the CLI / LSP do.
    let diags = ry_checker::filter_suppressed(c.take_diagnostics(), src);
    diags
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
        let mut got_codes: BTreeMap<&str, usize> = BTreeMap::new();
        for (code, _) in &got {
            *got_codes.entry(code.as_str()).or_insert(0) += 1;
        }
        let ok = match &fx.expected {
            Expectation::None => got.is_empty(),
            Expectation::Codes(expected) => {
                let mut want: BTreeMap<&str, usize> = BTreeMap::new();
                for c in expected {
                    *want.entry(c.as_str()).or_insert(0) += 1;
                }
                want == got_codes
            }
        };
        if ok {
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
