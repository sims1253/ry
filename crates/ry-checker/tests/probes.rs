//! Deterministic per-rule positive+negative probe matrix.
//!
//! For every rule the checker can emit (except the CLI-level file-heuristic
//! `RY097` and the retired `RY095`), this harness ships a **positive** probe
//! (source that *must* fire the rule) and a **negative** probe (source that
//! *must not*). The matrix is data-driven and runs entirely off the in-process
//! checker: no network, no `Rscript`, no installed packages. It complements
//! the `testdata/` corpus (which pins exact diagnostic *sets*) by pinning the
//! *direction* of each rule independently, so a regression that silently
//! widens or narrows a single rule is caught even when the corpus fixture's
//! exact code set happens to stay stable.
//!
//! Coverage is asserted: every rule in [`ry_checker::rules::RULES`] must have
//! a probe or be listed in [`EXCLUDED`] with a reason.

use ry_checker::{Checker, Severity};
use ry_core::RParser;

/// A single rule probe.
struct Probe {
    code: &'static str,
    /// Short note explaining the trigger shape, surfaced on failure.
    note: &'static str,
    positive: &'static str,
    negative: &'static str,
}

/// Run the single-file checker over `src` and return the emitted diagnostic
/// codes (with severity). Parse errors are kept: `RY000` is itself a probed
/// rule, and a recovered tree is exactly how the checker sees broken input.
fn run(src: &str) -> Vec<(&'static str, Severity)> {
    let mut parser = RParser::new().expect("parser init");
    // tree-sitter recovers from broken syntax, so `parse` succeeds and
    // records the errors on the file; only a catastrophic init failure
    // (out of memory) would fail here.
    let file = parser.parse("probe.R", src).expect("parse probe");
    let mut checker = Checker::new("probe.R");
    checker.check(&file);
    checker
        .take_diagnostics()
        .into_iter()
        .map(|d| (d.code, d.severity))
        .collect()
}

/// Rules intentionally absent from the matrix, each with a reason. These
/// are entries present in [`ry_checker::rules::RULES`] that the unit checker
/// cannot exercise. (Retired codes like `RY095` are already absent from
/// `RULES`, so they need no exclusion here.) Kept small: the goal is blanket
/// coverage, and every exclusion is a documented gap.
const EXCLUDED: &[(&str, &str)] = &[
    // CLI-level heuristic emitted by `ry-cli` from parser-recovery signals,
    // not by the checker, so it cannot be probed through `Checker::check`.
    ("RY097", "emitted by the CLI, not the checker"),
];

/// The probe matrix. Order follows the rule registry. When you add a rule to
/// `rules::RULES`, add a probe here and the coverage test stays green.
static PROBES: &[Probe] = &[
    Probe {
        code: "RY000",
        note: "unrecoverable syntax produces ERROR/MISSING nodes",
        positive: "f <- function( {\n  broken syntax ((\n",
        negative: "x <- 1L\n",
    },
    Probe {
        code: "RY001",
        note: "`if` on a non-logical atomic (character)",
        positive: "if (\"x\") print(1)\n",
        negative: "if (TRUE) print(1)\n",
    },
    Probe {
        code: "RY002",
        note: "`if` condition length > 1",
        positive: "if (c(TRUE, FALSE)) print(1)\n",
        negative: "if (TRUE) print(1)\n",
    },
    Probe {
        code: "RY003",
        note: "`if` on a bare numeric (implicit coercion)",
        positive: "if (1L) print(1)\n",
        negative: "if (TRUE) print(1)\n",
    },
    Probe {
        code: "RY010",
        note: "reference with no binding in scope",
        positive: "y <- undefined_thing\n",
        negative: "y <- 1L\n",
    },
    Probe {
        code: "RY020",
        note: "unary `-` on a non-numeric type",
        positive: "y <- -\"hello\"\n",
        negative: "y <- -1L\n",
    },
    Probe {
        code: "RY021",
        note: "unary `!` on a non-coercible type",
        positive: "y <- !\"hello\"\n",
        negative: "y <- !TRUE\n",
    },
    Probe {
        code: "RY030",
        note: "comparison of types with no defined ordering (function value)",
        positive: "f <- function() 1L\nbad <- f > 1L\n",
        negative: "x <- 1L\nok <- x > 0L\n",
    },
    Probe {
        code: "RY031",
        note: "`&`/`|` on a non-coercible (character) operand",
        positive: "y <- \"x\" & TRUE\n",
        negative: "y <- TRUE & FALSE\n",
    },
    Probe {
        code: "RY032",
        note: "scalar `&&`/`||` with a vector operand",
        positive: "x <- c(TRUE, FALSE, TRUE)\nbad <- x && TRUE\n",
        negative: "ok <- TRUE && FALSE\n",
    },
    Probe {
        code: "RY033",
        note: "character vs numeric comparison coerces lexicographically",
        positive: "bad <- \"hello\" < 42\n",
        negative: "ok <- 1L < 2L\n",
    },
    Probe {
        code: "RY034",
        note: "`==`/`!=` against NA is always NA",
        positive: "x <- 1L\nbad <- x == NA\n",
        negative: "x <- 1L\nok <- is.na(x)\n",
    },
    Probe {
        code: "RY040",
        note: "arithmetic between incompatible types",
        positive: "y <- \"a\" + 1L\n",
        negative: "y <- 1L + 2L\n",
    },
    Probe {
        code: "RY041",
        note: "vector lengths do not divide evenly",
        positive: "bad <- c(1, 2, 3) + c(10, 20)\n",
        negative: "ok <- c(1, 2) + c(10, 20)\n",
    },
    Probe {
        code: "RY042",
        note: "arithmetic on a factor produces NA",
        positive: "bad <- factor(c(\"a\", \"b\")) + 1\n",
        negative: "ok <- c(1, 2) + 1\n",
    },
    Probe {
        code: "RY050",
        note: "S3 generic with no method for the value's class",
        positive: "Summary.other <- function(...) 1L\n\
                   x <- structure(list(), class = \"undefined\")\n\
                   Summary(x)\n",
        negative: "Summary.undefined <- function(...) 1L\n\
                   x <- structure(list(), class = \"undefined\")\n\
                   Summary(x)\n",
    },
    Probe {
        code: "RY060",
        note: "column access not in a known data-frame schema",
        positive: "df <- mtcars\nbad <- df$nonexistent\n",
        negative: "df <- mtcars\nok <- df$mpg\n",
    },
    Probe {
        code: "RY061",
        note: "`$` on an atomic vector",
        positive: "x <- 1:10\nval <- x$column\n",
        negative: "x <- list(a = 1)\nval <- x$a\n",
    },
    Probe {
        code: "RY070",
        note: "calling a non-function value",
        positive: "x <- 42\ny <- x(10)\n",
        negative: "f <- function(x) x\ny <- f(10)\n",
    },
    Probe {
        code: "RY080",
        note: "purrr typed-map callback returns an incompatible mode",
        positive: "library(purrr)\nxs <- map_dbl(1:3, function(x) paste(\"n\", x))\n",
        negative: "library(purrr)\nxs <- map_dbl(1:3, function(x) x + 0.5)\n",
    },
    Probe {
        code: "RY090",
        note: "named argument matches no formal after exact/partial matching",
        positive: "length(xx = 1L)\n",
        negative: "c(a = 1, b = 2)\n",
    },
    Probe {
        code: "RY091",
        note: "a required formal is left unbound",
        positive: "length()\n",
        negative: "length(x = 1L)\n",
    },
    Probe {
        code: "RY092",
        note: "call argument mode incompatible with the parameter type",
        positive: "mean(\"not numeric\")\n",
        negative: "mean(1:10)\n",
    },
    Probe {
        code: "RY093",
        note: "comparison directly inside length()/nchar()",
        positive: "length(1 > 2)\n",
        negative: "length(1:3)\n",
    },
    Probe {
        code: "RY094",
        note: "literal printf format has more conversions than value args",
        positive: "sprintf(\"%d %d\", 1)\n",
        negative: "sprintf(\"%d %d\", 1, 2)\n",
    },
    Probe {
        code: "RY096",
        note: "hasArg() names a non-formal in a function without `...`",
        positive: "no_dots <- function(x) {\n  if (hasArg(threshold)) x <- x + 1\n  x\n}\n",
        negative: "with_dots <- function(x, ...) {\n  if (hasArg(threshold)) x <- x + 1\n  x\n}\n",
    },
    Probe {
        code: "RY098",
        note: "self-referential default forced before replacement",
        positive: "f <- function(x = x) if (TRUE) x else 1L\nf()\n",
        negative: "f <- function(x = 1L) x\nf()\n",
    },
    Probe {
        code: "RY099",
        note: "value-producing expr in a non-tail one-arm `if` is discarded",
        positive: "f <- function(z) {\n  if (z == 0) z + 0.001\n  z\n}\nf(1)\n",
        negative: "f <- function(x) {\n  if (x) x + 1 else x - 1\n}\nf(TRUE)\n",
    },
    Probe {
        code: "RY100",
        note: "comparison directly inside a numeric math call",
        positive: "x <- 1L\ny <- abs(x > 1L)\n",
        negative: "x <- 1L\ny <- abs(x)\n",
    },
    Probe {
        code: "RY101",
        note: "identical() of a single-bracket list subset with a scalar",
        positive: "args <- list(font = \"monospace\")\nbad <- identical(args[\"font\"], \"monospace\")\n",
        negative: "args <- list(font = \"monospace\")\nok <- identical(args[[\"font\"]], \"monospace\")\n",
    },
    Probe {
        code: "RY102",
        note: "`<-` where `=` was meant inside a name-carrying container",
        positive: "bad <- list(ref = \"a\", \"github-ref\" <- \"b\")\n",
        negative: "ok <- list(ref = \"a\", `github-ref` = \"b\")\n",
    },
    Probe {
        code: "RY103",
        note: "`class(x) ==` in a length-1 logical context",
        positive: "f <- function(x) if (class(x) == \"data.frame\") 1 else 2\n",
        negative: "f <- function(x) if (inherits(x, \"data.frame\")) 1 else 2\n",
    },
    Probe {
        code: "RY105",
        note: "`length()` of a length-1-by-construction value against 0",
        positive: "f <- function(v) if (length(sum(v)) > 0) 1 else 2\n",
        negative: "f <- function(v) if (length(v) > 0) 1 else 2\n",
    },
];

#[test]
fn positive_probes_fire_their_rule() {
    let mut failures = Vec::new();
    for probe in PROBES {
        let codes: Vec<&str> = run(probe.positive).into_iter().map(|(c, _)| c).collect();
        if !codes.contains(&probe.code) {
            failures.push(format!(
                "{} ({}): positive did not fire — got {:?}",
                probe.code, probe.note, codes
            ));
        }
    }
    if !failures.is_empty() {
        panic!(
            "probe matrix: {} positive probe(s) failed to fire:\n  - {}\n",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

#[test]
fn negative_probes_stay_silent_for_their_rule() {
    let mut failures = Vec::new();
    for probe in PROBES {
        let codes: Vec<&str> = run(probe.negative).into_iter().map(|(c, _)| c).collect();
        if codes.contains(&probe.code) {
            failures.push(format!(
                "{} ({}): negative wrongly fired — got {:?}",
                probe.code, probe.note, codes
            ));
        }
    }
    if !failures.is_empty() {
        panic!(
            "probe matrix: {} negative probe(s) fired unexpectedly:\n  - {}\n",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

#[test]
fn every_rule_has_a_probe_or_documented_exclusion() {
    let probed: std::collections::HashSet<&str> = PROBES.iter().map(|p| p.code).collect();
    let excluded: std::collections::HashSet<&str> = EXCLUDED.iter().map(|(c, _)| *c).collect();
    let mut missing = Vec::new();
    for rule in ry_checker::rules::RULES {
        if !probed.contains(rule.code) && !excluded.contains(rule.code) {
            missing.push(rule.code);
        }
    }
    // `EXCLUDED` must only name codes, and every exclusion must really be
    // absent from the matrix (otherwise the exclusion is dead documentation).
    let mut bogus = Vec::new();
    for (code, _) in EXCLUDED {
        if probed.contains(code) {
            bogus.push(*code);
        }
    }
    let mut unknown_exclusion = Vec::new();
    for (code, _) in EXCLUDED {
        if !ry_checker::rules::RULES.iter().any(|r| r.code == *code) {
            unknown_exclusion.push(*code);
        }
    }
    if !(missing.is_empty() && bogus.is_empty() && unknown_exclusion.is_empty()) {
        panic!(
            "probe matrix coverage broken:\n  missing probes: {:?}\n  probes also listed as excluded: {:?}\n  exclusions naming unknown rules: {:?}",
            missing, bogus, unknown_exclusion
        );
    }
}

/// Visibility no-op: print the matrix as a table for `cargo test probes_table
/// -- --nocapture` while iterating, mirroring the corpus summary helper.
#[test]
fn probes_table() {
    println!("{:<6} {:<48} positive fires?", "code", "note");
    println!("{}", "-".repeat(70));
    for probe in PROBES {
        let codes: Vec<&str> = run(probe.positive).into_iter().map(|(c, _)| c).collect();
        let fires = codes.contains(&probe.code);
        println!("{:<6} {:<48} {}", probe.code, probe.note, fires);
    }
}
