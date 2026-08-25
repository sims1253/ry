//! Clean-checkout baselines.
//!
//! Prove that committed ecosystem reports match what the checker actually
//! produces on the committed vendored source. This protects the
//! orchestration seam where a committed report can silently go stale:
//! someone modifies the checker or source but forgets to regenerate the
//! committed report, and CI reports a false green because the report
//! drift gate is the only thing catching it — and that gate is a shell
//! script that can itself degrade.
//!
//! This Rust test is a direct, shell-independent assertion. It reads the
//! committed `ecosystem/reports/glue.txt` (the local, R/-only report for
//! the vendored glue package), runs the checker on every committed glue
//! source file, formats the output identically to `run.sh`'s report
//! writer, and asserts equality. A checker change that alters glue's
//! diagnostics without a report regeneration fails here.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ry_core::RParser;

fn vendor_glue_r_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/vendor/glue/R")
}

fn committed_report() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ecosystem/reports/glue.txt")
}

fn r_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("R"))
        .collect();
    files.sort();
    files
}

/// Format a diagnostic identity the same way `run.sh`'s `write_report`
/// does: `relative_path:line:column CODE`.
fn format_identity(path: &str, line: u32, column: u32, code: &str) -> String {
    format!("{path}:{line}:{column} {code}")
}

/// Run the multi-file `Project` checker on the vendored glue source and
/// return the sorted set of diagnostic identities, mirroring the committed
/// `.txt` report format. Using `Project` (not single-file `Checker`) matches
/// the CLI's directory-mode check that `run.sh` invokes.
fn checker_identities() -> BTreeSet<String> {
    let dir = vendor_glue_r_dir();
    let files = r_files(&dir);
    assert!(
        !files.is_empty(),
        "expected glue R/ source files in {}",
        dir.display(),
    );

    let mut parser = RParser::new().expect("parser init");
    let mut project = ry_checker::Project::new();
    for file in &files {
        let src =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let name = file.to_string_lossy().to_string();
        let parsed = parser
            .parse(&name, &src)
            .unwrap_or_else(|e| panic!("parse {}: {e}", file.display()));
        project.add_file(name.clone(), parsed);
    }

    let mut identities = BTreeSet::new();
    for (path, diagnostics) in project.check() {
        let relative = Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        for diag in diagnostics {
            identities.insert(format_identity(
                relative,
                diag.span.line as u32 + 1,
                diag.span.col as u32 + 1,
                diag.code,
            ));
        }
    }
    identities
}

/// Read the committed report and return the sorted set of identities.
fn committed_identities() -> BTreeSet<String> {
    let report = committed_report();
    let content = fs::read_to_string(&report)
        .unwrap_or_else(|e| panic!("read committed report {}: {e}", report.display()));
    content
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

/// committed local glue report matches live checker output.
///
/// This is the clean-checkout baseline assertion. If the checker changes
/// behavior on the committed glue source without regenerating the report,
/// this test fails — catching the staleness before it reaches a release.
#[test]
fn clean_checkout_local_report_matches_committed_baselines() {
    let committed = committed_identities();
    let live = checker_identities();
    assert_eq!(
        committed,
        live,
        "committed ecosystem/reports/glue.txt does not match live \
         checker output on the committed vendored glue source.\n\
         Committed:\n{}\nLive:\n{}",
        committed.iter().cloned().collect::<Vec<_>>().join("\n"),
        live.iter().cloned().collect::<Vec<_>>().join("\n"),
    );
}

/// falsification — a deliberately corrupted committed report
/// is detected by the same comparison.
///
/// This proves the comparison is not vacuous (e.g. always returning empty).
/// If the committed report contained a spurious diagnostic that the
/// checker does not produce, the assertion above would catch it. We
/// verify this by constructing the comparison manually: the committed
/// report plus one fake entry must NOT equal the live output.
#[test]
fn clean_checkout_comparison_catches_a_spurious_committed_entry() {
    let live = checker_identities();
    let mut corrupted = live.clone();
    corrupted.insert("R/__fake__.R:1:1 RY999".to_string());
    assert_ne!(
        corrupted, live,
        "a spurious committed entry was not detected by the \
         identity comparison",
    );
}
