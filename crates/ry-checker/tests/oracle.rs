//! R oracle harness (PLAN.md Phase 0.5 / Phase 7).
//!
//! `#[ignore]`'d by default; run with
//! `cargo test -p ry-checker --test oracle -- --ignored --nocapture`
//! (or `scripts/oracle.sh` once Phase 7 adds it).
//!
//! For each fixture in `testdata/oracle/`, if `Rscript` is on PATH, runs
//! `Rscript --vanilla <file>`, records whether R errored, runs the checker,
//! and asserts:
//!   - `# oracle: must-flag` + R errored   => at least one Error diag.
//!   - `# oracle: must-pass` + R succeeded => no Error diag.
//!
//! Skips cleanly (returns) when `Rscript` is not installed.

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use ry_checker::{Checker, Severity};
use ry_core::RParser;

enum Tag {
    MustFlag,
    MustPass,
}

fn tag_of(src: &str) -> Option<Tag> {
    let first = src.lines().next()?;
    let trimmed = first
        .trim_start_matches([' ', '\t'])
        .trim_start_matches('#')
        .trim();
    if trimmed.eq_ignore_ascii_case("oracle: must-flag") {
        Some(Tag::MustFlag)
    } else if trimmed.eq_ignore_ascii_case("oracle: must-pass") {
        Some(Tag::MustPass)
    } else {
        None
    }
}

fn rscript_on_path() -> bool {
    which("Rscript").is_some()
}

fn which(prog: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(prog);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Returns true if R errored on this file (nonzero exit or "Error" on stderr).
fn r_errors(path: &std::path::Path) -> bool {
    let output = match Command::new("Rscript").arg("--vanilla").arg(path).output() {
        Ok(o) => o,
        Err(_) => return true, // treat missing/failed invocation conservatively
    };
    if !output.status.success() {
        return true;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("Error")
}

fn checker_errors(name: &str, src: &str) -> Vec<String> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse(name, src)
        .unwrap_or_else(|e| panic!("parse {name}: {e}"));
    let mut c = Checker::new(name);
    c.check(&file);
    let diags = c.take_diagnostics();
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code.to_string())
        .collect()
}

#[test]
#[ignore]
fn oracle_check_each_fixture() {
    if !rscript_on_path() {
        eprintln!("Rscript not on PATH; skipping oracle suite.");
        return;
    }

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/oracle");
    let mut entries: Vec<_> = match fs::read_dir(&dir) {
        Ok(e) => e.flatten().collect(),
        Err(_) => {
            eprintln!("no oracle dir at {}; skipping.", dir.display());
            return;
        }
    };
    entries.sort_by_key(|e| e.path());

    let mut failures: Vec<String> = Vec::new();
    let mut total: usize = 0;
    let mut passed: usize = 0;

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("R") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let src = fs::read_to_string(&path).expect("read fixture");
        let Some(tag) = tag_of(&src) else {
            failures.push(format!(
                "{name}: missing `# oracle: must-flag` / `must-pass` marker"
            ));
            continue;
        };
        total += 1;

        let r_errored = r_errors(&path);
        let errs = checker_errors(&name, &src);
        let mut err_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &errs {
            *err_counts.entry(c.as_str()).or_insert(0) += 1;
        }

        let ok = match (&tag, r_errored) {
            (Tag::MustFlag, true) => !errs.is_empty(),
            (Tag::MustPass, false) => errs.is_empty(),
            (Tag::MustFlag, false) => {
                failures.push(format!(
                    "{name}: tagged must-flag but R did not error; cannot assert"
                ));
                continue;
            }
            (Tag::MustPass, true) => {
                failures.push(format!(
                    "{name}: tagged must-pass but R errored; cannot assert"
                ));
                continue;
            }
        };

        if ok {
            passed += 1;
        } else {
            failures.push(format!(
                "{name}: tag={:?} r_errored={r_errored} err_codes={:?}",
                match tag {
                    Tag::MustFlag => "must-flag",
                    Tag::MustPass => "must-pass",
                },
                err_counts
            ));
        }
    }

    eprintln!("oracle: {passed}/{total} fixtures satisfied the oracle");
    if !failures.is_empty() {
        panic!(
            "oracle: {}/{} fixtures failed:\n  - {}\n",
            failures.len(),
            total,
            failures.join("\n  - ")
        );
    }
}
