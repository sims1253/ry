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
//!   - `# oracle: known-gap <reason>`      => runs; the delta is printed
//!     but does NOT fail. It DOES fail if the gap unexpectedly closes
//!     (ry and R now agree) -- a stale tag.
//!
//! Skips cleanly (returns) when `Rscript` is not installed.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::process::Command;

use ry_checker::{Checker, Severity};
use ry_core::RParser;

#[derive(Debug)]
enum Tag {
    MustFlag,
    MustPass,
    /// A genuine current gap. The one-line reason documents why ry and R
    /// disagree today; the harness prints the delta but does not fail on
    /// it. A stale tag (the gap has closed) DOES fail.
    KnownGap(String),
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
        // `# oracle: known-gap <reason>` -- the rest of the line after
        // the tag prefix is the free-text reason. Match the prefix
        // case-insensitively but keep the reason's original casing.
        let prefix = "oracle: known-gap";
        if trimmed.to_ascii_lowercase().starts_with(prefix) {
            let reason = trimmed[prefix.len()..].trim().to_string();
            Some(Tag::KnownGap(reason))
        } else {
            None
        }
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

/// Run the parallel oracle driver once over the whole fixture directory
/// and return a map from fixture filename to whether R errored on it.
/// Returns `None` if the driver could not run (missing purrr/mirai, bad
/// exit) -- the caller falls back to the serial per-fixture path.
///
/// The driver emits one JSON object per line on stdout
/// (`{"file":..,"errored":..,"message":..}`); errors are reported
/// structurally so the old stderr-contains-"Error" heuristic is no
/// longer needed (a latent locale-dependent bug in the serial path).
fn r_errors_via_driver(fixture_dir: &std::path::Path) -> Option<HashMap<String, bool>> {
    let driver = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("oracle_driver.R");
    let output = Command::new("Rscript")
        .arg(&driver)
        .arg(fixture_dir)
        .output()
        .ok()?;
    // The driver exits 3 to signal "purrr/mirai not installed"; treat
    // that as "unavailable" (None) so the caller falls back to serial.
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        // Minimal JSON parse: {"file":"<name>","errored":<bool>,...}.
        // Pull out the file and errored fields without a JSON dep.
        if let (Some(file), Some(errored)) = (extract_json_field(line, "file"), extract_json_bool(line, "errored")) {
            map.insert(file, errored);
        }
    }
    Some(map)
}

/// Extract the string value of `"<field>"` from a flat JSON line.
fn extract_json_field(line: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\":\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(esc) = chars.next() {
                match esc {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

/// Extract a boolean field's value from a flat JSON line.
fn extract_json_bool(line: &str, field: &str) -> Option<bool> {
    let needle = format!("\"{field}\":");
    let start = line.find(&needle)? + needle.len();
    let rest = line[start..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
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

    // PLAN Phase 3.3: prefer the parallel oracle driver (a single
    // Rscript invocation that evaluates every fixture via purrr::map +
    // mirai::in_parallel, dogfooding the very pattern the tool checks).
    // Fall back to the serial per-fixture Rscript path when purrr/mirai
    // are not installed or the driver fails.
    let driver_map = r_errors_via_driver(&dir);
    if driver_map.is_some() {
        eprintln!("oracle: using parallel driver (purrr + mirai)");
    } else {
        eprintln!("oracle: parallel driver unavailable; using serial per-fixture Rscript path");
    }

    let mut failures: Vec<String> = Vec::new();
    let mut total: usize = 0;
    let mut passed: usize = 0;
    let mut gaps: usize = 0;

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
                "{name}: missing `# oracle: must-flag` / `must-pass` / `known-gap` marker"
            ));
            continue;
        };
        total += 1;

        let r_errored = match &driver_map {
            Some(map) => map.get(&name).copied().unwrap_or(true),
            None => r_errors(&path),
        };
        let errs = checker_errors(&name, &src);
        let mut err_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &errs {
            *err_counts.entry(c.as_str()).or_insert(0) += 1;
        }

        if let Tag::KnownGap(reason) = &tag {
            // A known-gap is expected to disagree with R today. The
            // harness prints the delta but does NOT fail on it. It
            // DOES fail if the gap has closed (ry and R now agree),
            // i.e. the tag is stale and should be removed.
            //
            // "Agree" means: R errored AND ry flagged (the would-be
            // `must-flag` outcome), or R succeeded AND ry was silent
            // (the would-be `must-pass` outcome).
            let agrees = match r_errored {
                true => !errs.is_empty(),
                false => errs.is_empty(),
            };
            if agrees {
                failures.push(format!(
                    "{name}: STALE known-gap tag -- the gap has closed \
                     (ry and R now agree). Remove the `known-gap` marker \
                     and re-tag as `must-flag`/`must-pass`. \
                     (reason was: {reason:?}; r_errored={r_errored}, \
                     err_codes={err_counts:?})"
                ));
            } else {
                gaps += 1;
                eprintln!(
                    "oracle: known-gap {name} (reason: {reason:?}; \
                     r_errored={r_errored}, err_codes={err_counts:?})"
                );
            }
            // known-gap fixtures never count toward `passed`.
            continue;
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
            (Tag::KnownGap(_), _) => unreachable!("handled above"),
        };

        if ok {
            passed += 1;
        } else {
            failures.push(format!(
                "{name}: tag={} r_errored={r_errored} err_codes={:?}",
                tag_label(&tag),
                err_counts
            ));
        }
    }

    eprintln!("oracle: {passed}/{total} fixtures satisfied the oracle ({gaps} known gap(s))");
    if !failures.is_empty() {
        panic!(
            "oracle: {}/{} fixtures failed:\n  - {}\n",
            failures.len(),
            total,
            failures.join("\n  - ")
        );
    }
}

fn tag_label(tag: &Tag) -> &'static str {
    match tag {
        Tag::MustFlag => "must-flag",
        Tag::MustPass => "must-pass",
        Tag::KnownGap(_) => "known-gap",
    }
}

/// Unit tests for the marker parser. These do NOT require R, so they run
/// in the default (non-`--ignored`) gate and lock in the tag grammar
/// (including the `known-gap` prefix match and reason-casing behavior)
/// that the R-dependent `oracle_check_each_fixture` harness relies on.
#[test]
fn tag_of_parses_all_markers() {
    assert!(matches!(
        tag_of("# oracle: must-pass\n"),
        Some(Tag::MustPass)
    ));
    assert!(matches!(
        tag_of("# oracle: must-flag\n"),
        Some(Tag::MustFlag)
    ));
    // Unrecognized first line -> None (the harness treats this as a
    // missing-marker failure).
    assert!(tag_of("# just a comment\n").is_none());
    assert!(tag_of("x <- 1\n").is_none());
}

#[test]
fn tag_of_parses_known_gap_with_reason() {
    match tag_of("# oracle: known-gap ry does not model Foo()\n") {
        Some(Tag::KnownGap(reason)) => {
            // Reason keeps its original casing and full text.
            assert_eq!(reason, "ry does not model Foo()");
        }
        other => panic!("expected KnownGap, got {other:?}"),
    }
}

#[test]
fn tag_of_known_gap_is_case_insensitive_on_prefix() {
    // The `oracle: known-gap` prefix matches case-insensitively...
    match tag_of("# Oracle: KNOWN-GAP some reason\n") {
        Some(Tag::KnownGap(reason)) => assert_eq!(reason, "some reason"),
        other => panic!("expected KnownGap, got {other:?}"),
    }
}

#[test]
fn tag_of_known_gap_tolerates_leading_whitespace() {
    match tag_of("  # oracle: known-gap spaced\n") {
        Some(Tag::KnownGap(reason)) => assert_eq!(reason, "spaced"),
        other => panic!("expected KnownGap, got {other:?}"),
    }
}
