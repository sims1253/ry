//! Rule reachability, targeted mutations, and verdicts.
//!
//! Three deliverables:
//!
//! 1. **R7 literal-to-parameter lifting report** -- for every rule family where
//!    a triggering value can flow through a parameter, construct a pair:
//!    `f(literal)` (call with a literal argument) vs `f()` with
//!    `function(x = literal)` (the same literal as a parameter default). A rule
//!    that fires on the default form but not the call form is
//!    *lift-reachable* (it catches the defect when the developer writes the
//!    default but not when they pass the literal at the call site). A rule
//!    that fires on neither form is *parameter-unreachable* -- it can only
//!    fire on bare inline literals. A rule that fires on both is *consistent*.
//!    Syntactic rules (pattern checks that don't depend on parameter types)
//!    are marked *n/a*.
//!
//! 2. **Targeted mutation pilot** -- for RY032 and representative rule
//!    families, each mutation has a parse-clean assertion, a deterministic
//!    before/after diagnostic inventory, and a negative control.
//!
//! 3. **Verdict enforcement** -- every rule has an evidence-backed verdict in
//!    `docs/corpus/rule-evidence-0.9.md`. Tests verify the table is complete
//!    and that code-level verdicts (default-off) match the registry.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ry_checker::Checker;
use ry_checker::rules::{RULES, enabled_by_default};
use ry_core::RParser;
use ry_core::ast::{Expr, Stmt};

// -- Shared helpers --------------------------------------------------------

fn check_codes(src: &str) -> BTreeSet<String> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("evidence.R", src).expect("parse");
    let mut checker = Checker::new("evidence.R");
    checker.check(&file);
    checker
        .take_diagnostics()
        .into_iter()
        .map(|d| d.code.to_string())
        .collect()
}

fn parses_clean(src: &str) -> bool {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("evidence.R", src).expect("parse");
    file.parse_errors.is_empty()
}

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

// =======================================================================
// Deliverable 1: R7 literal-to-parameter lifting report
// =======================================================================

/// Per-rule R7 cases. Each case tests whether the rule fires identically
/// when the triggering value is a literal call argument vs a parameter
/// default.
struct R7Case {
    rule: &'static str,
    literal_src: &'static str,
    default_src: &'static str,
}

const R7_CASES: &[R7Case] = &[
    R7Case {
        rule: "RY001",
        literal_src: "f <- function(x) if (x) 1\nf(\"text\")\n",
        default_src: "f <- function(x = \"text\") if (x) 1\nf()\n",
    },
    R7Case {
        rule: "RY002",
        literal_src: "f <- function(x) if (x) 1\nf(c(TRUE, FALSE))\n",
        default_src: "f <- function(x = c(TRUE, FALSE)) if (x) 1\nf()\n",
    },
    R7Case {
        rule: "RY003",
        literal_src: "f <- function(x) if (x) 1\nf(1L)\n",
        default_src: "f <- function(x = 1L) if (x) 1\nf()\n",
    },
    R7Case {
        rule: "RY020",
        literal_src: "f <- function(x) -x\nf(\"text\")\n",
        default_src: "f <- function(x = \"text\") -x\nf()\n",
    },
    R7Case {
        rule: "RY021",
        literal_src: "f <- function(x) !x\nf(\"text\")\n",
        default_src: "f <- function(x = \"text\") !x\nf()\n",
    },
    R7Case {
        rule: "RY030",
        literal_src: "g <- function() 1L\nf <- function(x) x > g()\nf(\"text\")\n",
        default_src: "g <- function() 1L\nf <- function(x = \"text\") x > g()\nf()\n",
    },
    R7Case {
        rule: "RY031",
        literal_src: "f <- function(x) x & TRUE\nf(\"text\")\n",
        default_src: "f <- function(x = \"text\") x & TRUE\nf()\n",
    },
    R7Case {
        rule: "RY032",
        literal_src: "f <- function(x) x && TRUE\nf(c(TRUE, FALSE))\n",
        default_src: "f <- function(x = c(TRUE, FALSE)) x && TRUE\nf()\n",
    },
    R7Case {
        rule: "RY033",
        literal_src: "f <- function(x) x < 42\nf(\"hello\")\n",
        default_src: "f <- function(x = \"hello\") x < 42\nf()\n",
    },
    R7Case {
        rule: "RY034",
        literal_src: "f <- function(x) x == NA\nf(1L)\n",
        default_src: "f <- function(x = 1L) x == NA\nf()\n",
    },
    R7Case {
        rule: "RY040",
        literal_src: "f <- function(x) x + 1L\nf(\"text\")\n",
        default_src: "f <- function(x = \"text\") x + 1L\nf()\n",
    },
    R7Case {
        rule: "RY061",
        literal_src: "f <- function(x) x$col\nf(1:10)\n",
        default_src: "f <- function(x = 1:10) x$col\nf()\n",
    },
    R7Case {
        rule: "RY093",
        literal_src: "f <- function(x) length(x > 0L)\nf(1L)\n",
        default_src: "f <- function(x = 1L) length(x > 0L)\nf()\n",
    },
    R7Case {
        rule: "RY099",
        literal_src: "f <- function(z) {\n  if (z == 0) z + 0.001\n  z\n}\nf(1)\n",
        default_src: "f <- function(z = 1) {\n  if (z == 0) z + 0.001\n  z\n}\nf()\n",
    },
    R7Case {
        rule: "RY100",
        literal_src: "f <- function(x) abs(x > 1L)\nf(1L)\n",
        default_src: "f <- function(x = 1L) abs(x > 1L)\nf()\n",
    },
    R7Case {
        rule: "RY103",
        literal_src: "f <- function(x) if (class(x) == \"df\") 1 else 2\nf(1L)\n",
        default_src: "f <- function(x = 1L) if (class(x) == \"df\") 1 else 2\nf()\n",
    },
    R7Case {
        rule: "RY105",
        literal_src: "f <- function(v) if (length(sum(v)) > 0) 1 else 2\nf(1:3)\n",
        default_src: "f <- function(v = 1:3) if (length(sum(v)) > 0) 1 else 2\nf()\n",
    },
];

/// Rules for which R7 is not applicable: purely syntactic or structural.
const R7_NA_RULES: &[&str] = &[
    "RY000", "RY010", "RY041", "RY042", "RY050", "RY060", "RY070", "RY080", "RY090", "RY091",
    "RY092", "RY094", "RY096", "RY097", "RY098", "RY101", "RY102",
];

/// Run R7 over all applicable rule families and report the classification.
/// No assertion on zero divergences: this is report mode. The published
/// report lives in `docs/corpus/rule-evidence-0.9.md`.
#[test]
fn r7_literal_lift_report_over_rule_families() {
    let mut total = 0usize;
    let mut lift_reachable = Vec::new();
    let mut param_unreachable = Vec::new();
    let mut consistent = Vec::new();
    let mut call_only = Vec::new();

    for case in R7_CASES {
        let lit_codes = check_codes(case.literal_src);
        let def_codes = check_codes(case.default_src);
        total += 1;

        let lit_fires = lit_codes.contains(case.rule);
        let def_fires = def_codes.contains(case.rule);

        if lit_fires && def_fires {
            consistent.push(case.rule);
        } else if def_fires && !lit_fires {
            lift_reachable.push(case.rule);
        } else if lit_fires && !def_fires {
            call_only.push(case.rule);
        } else {
            param_unreachable.push(case.rule);
        }

        eprintln!(
            "R7 {}: lit_fires={lit_fires} def_fires={def_fires} lit={lit_codes:?} def={def_codes:?}",
            case.rule,
        );
    }

    eprintln!(
        "R7 report: {total} rule families checked\n  lift-reachable (default-only): {lift_reachable:?}\n  call-only: {call_only:?}\n  parameter-unreachable: {param_unreachable:?}\n  consistent: {consistent:?}\n  n/a (syntactic): {R7_NA_RULES:?}",
    );

    let covered: BTreeSet<&str> = R7_CASES.iter().map(|c| c.rule).collect();
    let na: BTreeSet<&str> = R7_NA_RULES.iter().copied().collect();
    let all_codes: BTreeSet<&str> = RULES.iter().map(|r| r.code).collect();
    let accounted: BTreeSet<&str> = covered.union(&na).copied().collect();
    let missing: Vec<&&str> = all_codes.difference(&accounted).collect();
    assert!(
        missing.is_empty(),
        "R7 coverage incomplete: rules without a case or n/a marker: {missing:?}",
    );
}

/// Scan checker testdata fixtures for natural R7 cases: single-parameter
/// functions called with a scalar literal argument. Lifts the literal to a
/// default and compares diagnostics.
#[test]
fn r7_literal_lift_fixture_scan() {
    let fixtures = checker_fixtures();
    assert!(fixtures.len() >= 229, "expected at least 229 fixtures");
    let mut parser = RParser::new().expect("parser init");
    let mut total_lifts = 0usize;
    let mut divergences = Vec::new();

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

        let fn_defs = collect_function_defs(&file.stmts);
        if fn_defs.is_empty() {
            continue;
        }

        let calls = collect_literal_calls(&file.stmts);
        if calls.is_empty() {
            continue;
        }

        let orig_codes = check_codes(&src);
        for (fn_name, param_name, has_default) in &fn_defs {
            if *has_default {
                continue; // parameter already has a default
            }
            for (call_name, lit_text) in &calls {
                if call_name != fn_name {
                    continue;
                }
                let lifted = construct_lifted_source(&src, fn_name, param_name, lit_text);
                if let Some(lifted) = lifted {
                    let lifted_codes = check_codes(&lifted);
                    total_lifts += 1;
                    if orig_codes != lifted_codes {
                        divergences.push(format!(
                            "{}: lift {}({}) diverged\n  orig: {orig_codes:?}\n  lifted: {lifted_codes:?}",
                            path.file_name().unwrap().to_string_lossy(),
                            fn_name, lit_text,
                        ));
                    }
                }
            }
        }
    }

    eprintln!(
        "R7 fixture scan: {total_lifts} natural lifts, {} divergences",
        divergences.len(),
    );
    for d in &divergences {
        eprintln!("  {d}");
    }
}

fn collect_function_defs(stmts: &[Stmt]) -> Vec<(String, String, bool)> {
    let mut defs = Vec::new();
    for stmt in stmts {
        if let Stmt::Assign {
            target: Expr::Ident { name, .. },
            value: Expr::Function { params, .. },
            ..
        } = stmt
            && let Some(first) = params.first()
        {
            defs.push((name.clone(), first.name.clone(), first.default.is_some()));
        }
    }
    defs
}

fn collect_literal_calls(stmts: &[Stmt]) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    for stmt in stmts {
        collect_calls_from_stmt(stmt, &mut calls);
    }
    calls
}

fn collect_calls_from_stmt(stmt: &Stmt, calls: &mut Vec<(String, String)>) {
    match stmt {
        Stmt::Expr(expr) => collect_calls_from_expr(expr, calls),
        Stmt::Assign { target, value, .. } => {
            collect_calls_from_expr(target, calls);
            collect_calls_from_expr(value, calls);
        }
        _ => {}
    }
}

fn collect_calls_from_expr(expr: &Expr, calls: &mut Vec<(String, String)>) {
    if let Expr::Call { func, args, .. } = expr
        && let Expr::Ident { name, .. } = func.as_ref()
        && args.len() == 1
        && let Some(lit) = literal_text(&args[0].value)
    {
        calls.push((name.clone(), lit));
    }
}

fn literal_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Logical(v, _) => Some(if *v { "TRUE" } else { "FALSE" }.to_string()),
        Expr::Integer(v, _) => Some(format!("{v}L")),
        Expr::Double(v, _) => Some(v.to_string()),
        Expr::String(s, _) => {
            let escaped = s.replace("\\", "\\\\").replace("\"", "\\\"");
            Some(format!("\"{escaped}\""))
        }
        Expr::Null(_) => Some("NULL".to_string()),
        _ => None,
    }
}

fn construct_lifted_source(
    src: &str,
    fn_name: &str,
    param_name: &str,
    lit_text: &str,
) -> Option<String> {
    // Best-effort: find the function definition and add the default, then
    // remove the argument from the call. Only handles the simple
    // single-statement pattern. Returns None for anything complex.
    let def_pattern = format!("function({param_name})");
    let def_replacement = format!("function({param_name} = {lit_text})");
    let call_pattern = format!("{fn_name}({lit_text})");
    let call_replacement = format!("{fn_name}()");
    if !src.contains(&def_pattern) || !src.contains(&call_pattern) {
        return None;
    }
    let result = src.replacen(&def_pattern, &def_replacement, 1);
    let result = result.replacen(&call_pattern, &call_replacement, 1);
    Some(result)
}

// =======================================================================
// Deliverable 2: Targeted mutation pilot
// =======================================================================

struct MutationCase {
    rule: &'static str,
    family: &'static str,
    original: &'static str,
    killed: &'static str,
    negative_control: &'static str,
}

const MUTATION_PILOT: &[MutationCase] = &[
    MutationCase {
        rule: "RY032",
        family: "scalar-logical-length",
        original: "x <- c(TRUE, FALSE)\nbad <- x && TRUE\n",
        killed: "x <- TRUE\nbad <- x && TRUE\n",
        negative_control: "x <- c(TRUE, FALSE)\nbad <- x && FALSE\n",
    },
    MutationCase {
        rule: "RY040",
        family: "invalid-arithmetic",
        original: "y <- \"text\" + 1L\n",
        killed: "y <- 1L + 1L\n",
        negative_control: "y <- \"text\" + 2L\n",
    },
    MutationCase {
        rule: "RY093",
        family: "comparison-inside-length",
        original: "bad <- length(x > 0L)\n",
        killed: "bad <- length(x) > 0L\n",
        negative_control: "bad <- length(x > 1L)\n",
    },
    MutationCase {
        rule: "RY103",
        family: "class-equality",
        original: "f <- function(x) if (class(x) == \"df\") 1 else 2\n",
        killed: "f <- function(x) if (inherits(x, \"df\")) 1 else 2\n",
        negative_control: "f <- function(x) if (class(x) == \"lm\") 1 else 2\n",
    },
];

/// The mutation pilot: each case verifies parse-clean assertion, deterministic
/// inventory, and negative control.
#[test]
fn mutation_pilot_distinguishes_broken_rule_from_broken_mutation() {
    for case in MUTATION_PILOT {
        // 1. Parse-clean assertion.
        assert!(
            parses_clean(case.original),
            "{}: original has parse errors",
            case.rule
        );
        assert!(
            parses_clean(case.killed),
            "{}: killed has parse errors",
            case.rule
        );
        assert!(
            parses_clean(case.negative_control),
            "{}: negative control has parse errors",
            case.rule
        );

        // 2. Deterministic inventory: original fires, killed does not.
        let orig_codes = check_codes(case.original);
        assert!(
            orig_codes.contains(case.rule),
            "{} ({}) original did not fire: {orig_codes:?}",
            case.rule,
            case.family
        );

        let killed_codes = check_codes(case.killed);
        assert!(
            !killed_codes.contains(case.rule),
            "{} ({}) kill mutation still fires: {killed_codes:?}",
            case.rule,
            case.family
        );

        // 3. Negative control: control still fires.
        let control_codes = check_codes(case.negative_control);
        assert!(
            control_codes.contains(case.rule),
            "{} ({}) negative control did not fire: {control_codes:?}",
            case.rule,
            case.family
        );

        eprintln!(
            "{} ({}): kill suppressed, negative control preserved",
            case.rule, case.family
        );
    }
}

/// RY032 standing case: policy is that unknown parameter length is
/// not evidence that &&/|| discards elements. R7 reports RY032 as
/// parameter-unreachable and that report IS the expected outcome.
#[test]
fn ry032_standing_case_parameter_is_not_literal_only_actionable() {
    let src = "f <- function(x) x && TRUE\n";
    let codes = check_codes(src);
    assert!(
        !codes.contains("RY032"),
        "RY032 fired on bare parameter with unknown length -- violates policy: {codes:?}"
    );

    let lit_src = "x <- c(TRUE, FALSE)\nbad <- x && TRUE\n";
    let lit_codes = check_codes(lit_src);
    assert!(
        lit_codes.contains("RY032"),
        "RY032 should fire on known-length vector literal: {lit_codes:?}"
    );
}

// =======================================================================
// Deliverable 3: Verdict enforcement
// =======================================================================

// Evidence rationale for each verdict lives in
// `docs/corpus/rule-evidence-0.9.md`; only the enforced fields are
// kept here.
struct Verdict {
    code: &'static str,
    verdict: &'static str,
}

const VERDICTS: &[Verdict] = &[
    Verdict {
        code: "RY000",
        verdict: "keep",
    },
    Verdict {
        code: "RY001",
        verdict: "keep",
    },
    Verdict {
        code: "RY002",
        verdict: "keep",
    },
    Verdict {
        code: "RY003",
        verdict: "default-off",
    },
    Verdict {
        code: "RY010",
        verdict: "keep",
    },
    Verdict {
        code: "RY020",
        verdict: "keep",
    },
    Verdict {
        code: "RY021",
        verdict: "keep",
    },
    Verdict {
        code: "RY030",
        verdict: "keep",
    },
    Verdict {
        code: "RY031",
        verdict: "keep",
    },
    Verdict {
        code: "RY032",
        verdict: "keep",
    },
    Verdict {
        code: "RY033",
        verdict: "keep",
    },
    Verdict {
        code: "RY034",
        verdict: "keep",
    },
    Verdict {
        code: "RY040",
        verdict: "keep",
    },
    Verdict {
        code: "RY041",
        verdict: "keep",
    },
    Verdict {
        code: "RY042",
        verdict: "keep",
    },
    Verdict {
        code: "RY050",
        verdict: "keep",
    },
    Verdict {
        code: "RY060",
        verdict: "keep",
    },
    Verdict {
        code: "RY061",
        verdict: "keep",
    },
    Verdict {
        code: "RY070",
        verdict: "keep",
    },
    Verdict {
        code: "RY080",
        verdict: "keep",
    },
    Verdict {
        code: "RY090",
        verdict: "keep",
    },
    Verdict {
        code: "RY091",
        verdict: "keep",
    },
    Verdict {
        code: "RY092",
        verdict: "keep",
    },
    Verdict {
        code: "RY093",
        verdict: "keep",
    },
    Verdict {
        code: "RY094",
        verdict: "keep",
    },
    Verdict {
        code: "RY096",
        verdict: "keep",
    },
    Verdict {
        code: "RY097",
        verdict: "keep",
    },
    Verdict {
        code: "RY098",
        verdict: "keep",
    },
    Verdict {
        code: "RY099",
        verdict: "keep",
    },
    Verdict {
        code: "RY100",
        verdict: "keep",
    },
    Verdict {
        code: "RY101",
        verdict: "keep",
    },
    Verdict {
        code: "RY102",
        verdict: "keep",
    },
    Verdict {
        code: "RY103",
        verdict: "keep",
    },
    Verdict {
        code: "RY105",
        verdict: "keep",
    },
];

#[test]
fn every_rule_has_an_executed_verdict() {
    let verdict_codes: BTreeSet<&str> = VERDICTS.iter().map(|v| v.code).collect();
    let rule_codes: BTreeSet<&str> = RULES.iter().map(|r| r.code).collect();
    let missing: Vec<&&str> = rule_codes.difference(&verdict_codes).collect();
    let extra: Vec<&&str> = verdict_codes.difference(&rule_codes).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "verdict coverage broken: missing={missing:?} extra={extra:?}"
    );
}

#[test]
fn verdicts_use_only_allowed_values() {
    const ALLOWED: &[&str] = &["keep", "fix", "default-off", "retire"];
    for v in VERDICTS {
        assert!(
            ALLOWED.contains(&v.verdict),
            "{} has invalid verdict '{}': {:?}",
            v.code,
            v.verdict,
            ALLOWED
        );
    }
}

#[test]
fn default_off_verdicts_match_the_registry() {
    for v in VERDICTS {
        let registry_enabled = enabled_by_default(v.code);
        match v.verdict {
            "default-off" => assert!(
                !registry_enabled,
                "{} verdict is default-off but enabled_by_default returns true",
                v.code
            ),
            "keep" | "fix" => assert!(
                registry_enabled,
                "{} verdict is {} but enabled_by_default returns false",
                v.code, v.verdict
            ),
            "retire" => panic!("{} has retire verdict but is still in RULES", v.code),
            _ => unreachable!(),
        }
    }
}

#[test]
fn reverting_ry003_default_off_fails() {
    assert!(
        !enabled_by_default("RY003"),
        "RY003 must be disabled by default (default-off verdict)"
    );
    for rule in RULES {
        if rule.code == "RY003" {
            continue;
        }
        assert!(
            enabled_by_default(rule.code),
            "{} must be enabled by default (keep verdict)",
            rule.code
        );
    }
}

#[test]
fn retired_rules_are_absent_from_the_registry() {
    let codes: BTreeSet<&str> = RULES.iter().map(|r| r.code).collect();
    assert!(
        !codes.contains("RY095"),
        "RY095 is retired and must not be in RULES"
    );
}
