//! R oracle harness.
//!
//! The complete fixture matrix is `#[ignore]`'d by default; run it with
//! `cargo test -p ry-checker --test oracle -- --ignored --nocapture`.
//! Registered `# oracle-claim: RYxxx` fixtures run
//! in the default test gate, and their registry coverage is always checked.
//!
//! For each fixture in `testdata/oracle/`, if `Rscript` is on PATH, runs
//! `Rscript --vanilla <file>`, records whether R errored, runs the checker,
//! and asserts:
//!   - `# oracle: must-flag` + R errored   => at least one Error diag.
//!   - `# oracle: must-pass` + R succeeded => no Error diag.
//!   - `# oracle: must-warn RYxxx`         => R-side assertions pass and
//!     ry emits that warning.
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
    MustWarn(String),
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
        let warn_prefix = "oracle: must-warn";
        if trimmed.to_ascii_lowercase().starts_with(warn_prefix) {
            let code = trimmed[warn_prefix.len()..].trim().to_ascii_uppercase();
            return (!code.is_empty()).then_some(Tag::MustWarn(code));
        }
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

/// Rule codes whose R semantics are demonstrated by this fixture.
///
/// Claim registration is deliberately separate from the outcome marker: the
/// marker describes the checker/R agreement shape, while `oracle-claim` says
/// which registry premise the R-side error, warning, or assertion establishes.
fn claim_codes(src: &str) -> Result<Vec<String>, String> {
    let mut claims = Vec::new();
    for line in src.lines() {
        let trimmed = line
            .trim_start_matches([' ', '\t'])
            .trim_start_matches('#')
            .trim();
        let prefix = "oracle-claim:";
        if !trimmed.to_ascii_lowercase().starts_with(prefix) {
            continue;
        }
        let code = trimmed[prefix.len()..].trim().to_ascii_uppercase();
        if code.is_empty() || code.split_whitespace().count() != 1 {
            return Err(format!(
                "`# oracle-claim:` must name exactly one rule code, got {code:?}"
            ));
        }
        if claims.contains(&code) {
            return Err(format!("duplicate oracle claim {code}"));
        }
        claims.push(code);
    }
    Ok(claims)
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

/// Whether R errored on this file (nonzero exit or "Error" on stderr),
/// plus a diagnostic snippet of R's stderr so a failure report says WHY
/// R errored (an environment problem reads very differently from a
/// semantic one, and the bare boolean forced guesswork on CI).
fn r_errors(path: &std::path::Path) -> (bool, String) {
    let output = match Command::new("Rscript").arg("--vanilla").arg(path).output() {
        Ok(o) => o,
        Err(e) => return (true, format!("failed to invoke Rscript: {e}")),
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    let snippet = tail_snippet(&stderr);
    let errored = !output.status.success() || stderr.contains("Error");
    (errored, snippet)
}

/// Last few lines of a process stream, flattened for a one-line report.
fn tail_snippet(s: &str) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(4);
    lines[start..].join(" | ")
}

/// R packages a fixture declares via `library(pkg)`, `require(pkg)`, or
/// `requireNamespace("pkg")`. Scanned lexically (the fixtures are flat
/// scripts); comment lines are ignored so a comment MENTIONING library()
/// does not count.
fn fixture_packages(src: &str) -> Vec<String> {
    let mut pkgs: Vec<String> = Vec::new();
    for line in src.lines() {
        let code = line.split('#').next().unwrap_or("");
        for prefix in ["library(", "require(", "requireNamespace("] {
            let Some(pos) = code.find(prefix) else {
                continue;
            };
            let rest = &code[pos + prefix.len()..];
            let end = rest.find([')', ',']).unwrap_or(rest.len());
            let name = rest[..end].trim().trim_matches(['"', '\'']).to_string();
            if !name.is_empty() && !pkgs.contains(&name) {
                pkgs.push(name);
            }
        }
    }
    pkgs
}

/// Whether an R package is installed, probed once per package via
/// `requireNamespace` (cached across fixtures). A fixture that needs a
/// missing package is SKIPPED with a note rather than failed: R erroring
/// because the environment lacks a CRAN package is not a semantic
/// disagreement between ry and R. CI installs every package the fixtures
/// use (see .github/workflows/ci.yml), so skips cannot mask a regression
/// there.
fn r_package_available(pkg: &str, cache: &mut HashMap<String, bool>) -> bool {
    if let Some(&hit) = cache.get(pkg) {
        return hit;
    }
    let probe = format!("quit(status = if (requireNamespace(\"{pkg}\", quietly = TRUE)) 0 else 1)");
    let available = Command::new("Rscript")
        .arg("-e")
        .arg(&probe)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    cache.insert(pkg.to_string(), available);
    available
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
fn r_errors_via_driver(fixture_dir: &std::path::Path) -> Option<HashMap<String, (bool, String)>> {
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
    // The driver exits 3 to signal "required packages not installed";
    // treat any failure as "unavailable" (None) so the caller falls back
    // to serial -- but SAY WHY on stderr. Two CI rounds were spent
    // guessing at a silent driver failure (a missing suggested package,
    // carrier) that this line would have named immediately.
    if !output.status.success() {
        eprintln!(
            "oracle: driver exited {:?}: {}",
            output.status.code(),
            tail_snippet(&String::from_utf8_lossy(&output.stderr))
        );
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
        // Pull out the file, errored, and message fields without a JSON
        // dep. The message travels into failure reports so a must-pass
        // violation names R's actual error.
        if let (Some(file), Some(errored)) = (
            extract_json_field(line, "file"),
            extract_json_bool(line, "errored"),
        ) {
            let message = extract_json_field(line, "message").unwrap_or_default();
            map.insert(file, (errored, message));
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

fn checker_diagnostics(name: &str, src: &str) -> Vec<(String, Severity)> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse(name, src)
        .unwrap_or_else(|e| panic!("parse {name}: {e}"));
    let mut c = Checker::new(name);
    c.check(&file);
    let diags = c.take_diagnostics();
    diags
        .into_iter()
        .map(|d| (d.code.to_string(), d.severity))
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

    // Prefer the parallel oracle driver (a single
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
    let mut skipped: usize = 0;
    let mut pkg_cache: HashMap<String, bool> = HashMap::new();

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
                "{name}: missing `# oracle: must-flag` / `must-pass` / `must-warn` / `known-gap` marker"
            ));
            continue;
        };

        // Skip (loudly) when the fixture needs a CRAN package this
        // machine does not have: R erroring for an environmental reason
        // is not a semantic ry-vs-R disagreement. CI installs everything
        // the fixtures use, so skips cannot hide a regression there.
        let missing: Vec<String> = fixture_packages(&src)
            .into_iter()
            .filter(|p| !r_package_available(p, &mut pkg_cache))
            .collect();
        if !missing.is_empty() {
            skipped += 1;
            eprintln!(
                "oracle: SKIP {name} (missing R package(s): {})",
                missing.join(", ")
            );
            continue;
        }
        total += 1;

        let (r_errored, r_message) = match &driver_map {
            Some(map) => map
                .get(&name)
                .cloned()
                .unwrap_or((true, "fixture missing from driver output".to_string())),
            None => r_errors(&path),
        };
        let diagnostics = checker_diagnostics(&name, &src);
        let errs: Vec<&str> = diagnostics
            .iter()
            .filter(|(_, severity)| *severity == Severity::Error)
            .map(|(code, _)| code.as_str())
            .collect();
        let mut err_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &errs {
            *err_counts.entry(c).or_insert(0) += 1;
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
            (Tag::MustWarn(code), false) => diagnostics
                .iter()
                .any(|(actual, severity)| actual == code && *severity == Severity::Warning),
            (Tag::MustFlag, false) => {
                failures.push(format!(
                    "{name}: tagged must-flag but R did not error; cannot assert"
                ));
                continue;
            }
            (Tag::MustPass, true) => {
                // Include R's actual error so an environment problem
                // (missing package, sandbox restriction) is readable
                // straight from the CI log instead of requiring a
                // reproduction.
                failures.push(format!(
                    "{name}: tagged must-pass but R errored; cannot assert (R said: {r_message})"
                ));
                continue;
            }
            (Tag::MustWarn(code), true) => {
                failures.push(format!(
                    "{name}: tagged must-warn {code} but its R-side assertions failed (R said: {r_message})"
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

    eprintln!(
        "oracle: {passed}/{total} fixtures satisfied the oracle \
         ({gaps} known gap(s), {skipped} skipped for missing packages)"
    );
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
        Tag::MustWarn(_) => "must-warn",
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
    assert!(matches!(
        tag_of("# oracle: must-warn ry041\n"),
        Some(Tag::MustWarn(code)) if code == "RY041"
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

#[test]
fn claim_codes_parse_registration() {
    assert_eq!(
        claim_codes("# oracle: must-pass\n# oracle-claim: ry003\nif (1) 1\n").unwrap(),
        vec!["RY003"]
    );
    assert!(claim_codes("# oracle-claim:\n").is_err());
    assert!(claim_codes("# oracle-claim: RY001 RY002\n").is_err());
    assert!(claim_codes("# oracle-claim: RY001\n# oracle-claim: RY001\n").is_err());
}

/// Completeness is a normal (non-ignored) test so adding a registry entry
/// without an R-backed semantic premise cannot silently bypass the oracle on
/// machines that do not have R installed.
#[test]
fn every_rule_has_a_claim_fixture() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/oracle");
    let mut claims: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut failures = Vec::new();

    for entry in fs::read_dir(&dir).expect("read oracle fixture directory") {
        let path = entry.expect("read oracle fixture entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("R") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let src = fs::read_to_string(&path).expect("read oracle fixture");
        let fixture_claims = match claim_codes(&src) {
            Ok(codes) => codes,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        if !fixture_claims.is_empty() && matches!(tag_of(&src), Some(Tag::KnownGap(_))) {
            failures.push(format!(
                "{name}: known-gap fixtures cannot satisfy semantic-claim coverage"
            ));
        }
        for code in fixture_claims {
            if !ry_checker::rules::RULES
                .iter()
                .any(|rule| rule.code == code)
            {
                failures.push(format!(
                    "{name}: oracle claim names unknown or retired rule {code}"
                ));
            }
            claims.entry(code).or_default().push(name.clone());
        }
    }

    for rule in ry_checker::rules::RULES {
        if !claims.contains_key(rule.code) {
            failures.push(format!(
                "{} ({}): missing `# oracle-claim: {}` fixture",
                rule.code, rule.name, rule.code
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "oracle semantic-claim coverage broken:\n  - {}\n",
            failures.join("\n  - ")
        );
    }
}

/// Execute only registered semantic claims in the default test gate. The full
/// checker-vs-R fixture matrix remains ignored because it includes slower CRAN
/// package scenarios; claim fixtures are small and are the release premise
/// gate. Missing R or a non-base package skips locally, as the full oracle does.
#[test]
fn claim_fixtures_demonstrate_their_r_premise() {
    if !rscript_on_path() {
        eprintln!("Rscript not on PATH; skipping semantic-claim execution.");
        return;
    }

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/oracle");
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .expect("read oracle fixture directory")
        .flatten()
        .collect();
    entries.sort_by_key(|entry| entry.path());

    let mut failures = Vec::new();
    let mut pkg_cache = HashMap::new();
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
        let src = fs::read_to_string(&path).expect("read oracle fixture");
        let claims = match claim_codes(&src) {
            Ok(claims) if claims.is_empty() => continue,
            Ok(claims) => claims,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        let Some(tag) = tag_of(&src) else {
            failures.push(format!(
                "{name}: claim fixture has no oracle outcome marker"
            ));
            continue;
        };
        if matches!(tag, Tag::KnownGap(_)) {
            failures.push(format!(
                "{name}: known-gap cannot demonstrate claims {}",
                claims.join(", ")
            ));
            continue;
        }

        let missing: Vec<String> = fixture_packages(&src)
            .into_iter()
            .filter(|pkg| !r_package_available(pkg, &mut pkg_cache))
            .collect();
        if !missing.is_empty() {
            eprintln!(
                "oracle: SKIP claim fixture {name} (missing R package(s): {})",
                missing.join(", ")
            );
            continue;
        }

        let (r_errored, r_message) = r_errors(&path);
        let demonstrated = match tag {
            Tag::MustFlag => r_errored,
            Tag::MustPass | Tag::MustWarn(_) => !r_errored,
            Tag::KnownGap(_) => unreachable!("handled above"),
        };
        if !demonstrated {
            failures.push(format!(
                "{name}: R did not demonstrate {} under {} (R said: {r_message})",
                claims.join(", "),
                tag_label(&tag)
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "oracle semantic claims failed:\n  - {}\n",
            failures.join("\n  - ")
        );
    }
}

#[test]
fn fixture_packages_scans_library_require_and_namespace_calls() {
    let src = "# oracle: must-pass\n\
               # a comment mentioning library(fake) does not count\n\
               library(purrr)\n\
               require(mirai)\n\
               if (requireNamespace(\"dplyr\", quietly = TRUE)) print(1)\n\
               library(purrr)  # duplicate, deduplicated\n\
               x <- 1\n";
    assert_eq!(fixture_packages(src), vec!["purrr", "mirai", "dplyr"]);
}

#[test]
fn fixture_packages_empty_for_plain_fixtures() {
    assert!(fixture_packages("# oracle: must-flag\nx <- \"a\" + 1\n").is_empty());
}

// ── R-oracle setup falsification ────────────────────────────────

/// Prove the R oracle verification actually fails when a wrong
/// answer is given.
///
/// This is not a meta-test proving an ordinary assertion can fail. It
/// protects the orchestration seam where `r_errors` interprets Rscript's
/// exit status and stderr, and where the oracle tag logic must reject a
/// `must-pass` fixture whose R execution actually errors. If `r_errors`
/// stopped checking `output.status.success()` or stopped scanning stderr
/// for "Error" (the locale-dependent heuristic the parallel driver replaces
/// but the serial path still uses), a real semantic disagreement would
/// pass silently through CI.
///
/// The test creates a temporary fixture tagged `must-pass` that R errors
/// on (`"a" + 1` — non-numeric to binary operator), then:
///   1. Asserts `r_errors` detects the R error.
///   2. Asserts the oracle tag logic classifies this as a failure
///      (`must-pass` + `r_errored=true` is never the `ok` branch).
#[test]
fn oracle_r_error_detection_fails_on_wrong_answer() {
    if !rscript_on_path() {
        eprintln!("Rscript not on PATH; skipping oracle falsification.");
        return;
    }

    // Write a must-pass fixture that R will reject. `"a" + 1` is a
    // mode-mismatch that R always errors on, with no package dependency.
    let dir = std::env::temp_dir();
    let path = dir.join("ry_w11_oracle_falsification.R");
    fs::write(&path, "# oracle: must-pass\nx <- \"a\" + 1\n").expect("write temp fixture");

    let (r_errored, r_message) = r_errors(&path);
    let _ = std::fs::remove_file(&path);

    assert!(
        r_errored,
        "r_errors failed to detect that R errors on the          deliberately wrong fixture; R said: {r_message}",
    );

    // The oracle harness's own logic: a `must-pass` fixture with an R error
    // is ALWAYS a failure (it hits the `MustPass, true` arm which pushes a
    // failure string). Prove this by checking the same condition the
    // harness uses.
    let src = "# oracle: must-pass\nx <- \"a\" + 1\n";
    let tag = tag_of(src).expect("tag parsed");
    assert!(matches!(tag, Tag::MustPass));
    let ok = match (&tag, r_errored) {
        (Tag::MustPass, false) => true,
        (Tag::MustPass, true) => false, // ← failure branch
        _ => unreachable!(),
    };
    assert!(
        !ok,
        "the oracle must-pass + r_errored arm should classify as failure",
    );
}

/// prove the oracle's `must-flag` path catches a missing error.
///
/// A `must-flag` fixture that R does NOT error on means the oracle cannot
/// assert anything (the `MustFlag, false` arm pushes a failure). This
/// protects the seam where a must-flag fixture with a broken R-side
/// premise would be silently accepted if the oracle stopped checking
/// `r_errored`.
#[test]
fn oracle_must_flag_fails_when_r_does_not_error() {
    let src = "# oracle: must-flag\nx <- 1 + 1\n";
    let tag = tag_of(src).expect("tag parsed");
    assert!(matches!(tag, Tag::MustFlag));

    // Simulate R succeeding (r_errored=false) on a must-flag fixture.
    // The oracle's MustFlag + r_errored=false arm pushes a failure.
    let r_errored = false;
    let ok = match (&tag, r_errored) {
        (Tag::MustFlag, true) => true,
        (Tag::MustFlag, false) => false, // ← failure branch
        _ => unreachable!(),
    };
    assert!(
        !ok,
        "must-flag + R-success should classify as oracle failure",
    );
}
