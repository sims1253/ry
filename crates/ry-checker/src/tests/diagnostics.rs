use super::*;

// ---- inline suppression comment tests ----
#[test]
fn parse_trailing_ignore_comment() {
    let supps = parse_suppressions("x <- bad  # ry: ignore\n");
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 0);
    assert!(supps[0].rules.is_empty()); // suppress all
}

#[test]
fn parse_specific_rule_ignore() {
    let supps = parse_suppressions("x <- \"a\" * 3  # ry: ignore[RY040]\n");
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].rules, vec!["RY040"]);
}

#[test]
fn parse_multiple_rules() {
    let supps = parse_suppressions("x <- bad  # ry: ignore[RY040, RY010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY040".to_string()));
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_standalone_comment_applies_to_next_line() {
    let src = "# ry: ignore\nx <- bad\n";
    let supps = parse_suppressions(src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 1);
}

#[test]
fn parse_standalone_comment_skips_blank_lines() {
    let src = "# ry: ignore\n\nx <- bad\n";
    let supps = parse_suppressions(src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 2);
}

#[test]
fn parse_noqa_alias() {
    let supps = parse_suppressions("x <- bad  # noqa: RY010\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_bare_noqa_suppresses_all() {
    let supps = parse_suppressions("x <- bad  # noqa\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.is_empty());
}

#[test]
fn parse_noqa_bracket_form() {
    let supps = parse_suppressions("x <- bad  # noqa[RY010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_compact_ry_ignore_no_space() {
    let supps = parse_suppressions("x <- bad  # ry:ignore[RY010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_case_insensitive_marker() {
    let supps = parse_suppressions("x <- bad  # RY: IGNORE[ry010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_non_suppression_comment_is_ignored() {
    let supps = parse_suppressions("# just a regular comment\nx <- bad\n");
    assert!(supps.is_empty());
}

#[test]
fn parse_file_level_suppression() {
    assert!(has_file_suppression("# ry: ignore-file\nx <- bad\n"));
    assert!(has_file_suppression("# ry:ignore-file\nx <- bad\n"));
    assert!(!has_file_suppression("# ry: ignore\nx <- bad\n"));
}

#[test]
fn file_level_marker_not_treated_as_line_level() {
    // `# ry: ignore-file` must NOT also register as a line-level
    // "ignore all" (it's handled by has_file_suppression instead).
    let supps = parse_suppressions("# ry: ignore-file\nx <- bad\n");
    assert!(
        supps.is_empty(),
        "ignore-file should not produce line-level suppressions, got {:?}",
        supps
    );
}

#[test]
fn is_suppressed_matches_line_and_code() {
    let supps = vec![Suppression {
        line: 2,
        rules: vec!["RY010".to_string()],
    }];
    let diag_matching = Diagnostic {
        severity: Severity::Warning,
        span: Span {
            start: 0,
            end: 1,
            line: 2,
            col: 0,
        },
        path: "x.R".into(),
        code: "RY010",
        message: "test".into(),
        confidence: Confidence::Medium,
    };
    let diag_wrong_line = Diagnostic {
        span: Span {
            line: 0,
            ..diag_matching.span
        },
        ..diag_matching.clone()
    };
    let diag_wrong_code = Diagnostic {
        code: "RY040",
        ..diag_matching.clone()
    };
    assert!(is_suppressed(&diag_matching, &supps));
    assert!(!is_suppressed(&diag_wrong_line, &supps));
    assert!(!is_suppressed(&diag_wrong_code, &supps));
}

#[test]
fn is_suppressed_empty_rules_matches_any_code() {
    let supps = vec![Suppression {
        line: 0,
        rules: vec![],
    }];
    let diag = Diagnostic {
        severity: Severity::Warning,
        span: Span {
            start: 0,
            end: 1,
            line: 0,
            col: 0,
        },
        path: "x.R".into(),
        code: "RY999",
        message: "test".into(),
        confidence: Confidence::Medium,
    };
    assert!(is_suppressed(&diag, &supps));
}

#[test]
fn filter_suppressed_end_to_end() {
    // Trailing `# ry: ignore[RY010]` on the offending line drops RY010.
    let src = "x <- undefined_var  # ry: ignore[RY010]\n";
    let diags = check(src);
    let filtered = filter_suppressed(diags, src);
    assert!(
        filtered.iter().all(|d| d.code != "RY010"),
        "RY010 should be suppressed, got {:?}",
        filtered
    );
}

#[test]
fn filter_suppressed_file_level_drops_everything() {
    let src = "# ry: ignore-file\nx <- undefined_var\n";
    let diags = check(src);
    let filtered = filter_suppressed(diags, src);
    assert!(
        filtered.is_empty(),
        "file-level suppression should drop all diagnostics, got {:?}",
        filtered
    );
}

#[test]
fn filter_suppressed_other_rules_still_fire() {
    // Suppressing RY010 on line 0 should NOT affect RY040 on line 1.
    let src = "x <- undefined_var  # ry: ignore[RY010]\ny <- \"a\" * 3L\n";
    let diags = check(src);
    let filtered = filter_suppressed(diags, src);
    assert!(
        filtered.iter().any(|d| d.code == "RY040"),
        "RY040 should still fire (it's on a different line), got {:?}",
        filtered
    );
    assert!(
        filtered.iter().all(|d| d.code != "RY010"),
        "RY010 should be suppressed"
    );
}

/// Idempotence: running the checker twice on the same
/// input must yield identical diagnostics. The fixpoint/refinement
/// machinery walks function tables whose iteration order is not
/// semantically meaningful, so any order-leak that bleeds into
/// observed types would show up here.
#[test]
fn diagnostics_are_deterministic_across_runs() {
    let sources = [
        // recursion (cycle detection in the fixpoint)
        "f <- function(n) { if (n > 0) f(n - 1) else 0L }\nx <- f(3) + 1\n",
        // mutual / cross-referencing function bodies
        "f <- function() { g() }\ng <- function() { 1L }\nx <- f() + 1\n",
        // a body with an arithmetic error + unbound var (exercises the
        // function-body walk in both passes)
        "h <- function() { a <- \"x\" + 1; b <- missing_thing }\n",
        // higher-order callback inference
        "v <- sapply(c(1.0, 2.0), function(x) x * 2)\ny <- v + 1\n",
        // a clean file (no diagnostics) with a closure factory
        "make_adder <- function(x) function(y) x + y\nadd5 <- make_adder(5)\nz <- add5(3)\n",
    ];
    for src in sources {
        let d1 = check(src);
        let d2 = check(src);
        // Compare on the semantically meaningful fields; `Diagnostic`
        // also carries `path` (constant here) and `message` (stable).
        let key = |d: &Diagnostic| (d.code, d.severity, d.span.start, d.span.end);
        let k1: Vec<_> = d1.iter().map(key).collect();
        let k2: Vec<_> = d2.iter().map(key).collect();
        assert_eq!(
            k1, k2,
            "non-deterministic diagnostics for src={src:?}\n  run1={d1:?}\n  run2={d2:?}"
        );
    }
}

#[test]
fn public_check_with_scope_surfaces_ry000_on_broken_file() {
    // Regression: `check_with_scope` used to clear diagnostics
    // AFTER emitting parse errors, wiping the RY000s. It must now
    // surface them.
    let mut p = RParser::new().unwrap();
    let f = p.parse("test.R", "f <- function( { 1 }\n").unwrap();
    let mut c = Checker::new("test.R");
    let (diags, _scope) = c.check_with_scope(&f);
    assert!(
        diags.iter().any(|d| d.code == "RY000"),
        "check_with_scope must surface RY000 on a broken file, got {:?}",
        diags
    );
}

#[test]
fn public_check_emits_each_parse_error_once() {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", "f <- function( { 1 }\n").unwrap();
    let expected = file.parse_errors.len();
    assert!(expected > 0, "fixture must contain a parse error");
    let mut checker = Checker::new("test.R");
    let actual = checker
        .check(&file)
        .iter()
        .filter(|diagnostic| diagnostic.code == "RY000")
        .count();

    assert_eq!(
        actual, expected,
        "each parser error must produce exactly one RY000"
    );
}

// ---- comparison-in-call & format arity (moved from packages_typeshed) ----
#[test]
fn comparison_directly_inside_length_is_diagnosed() {
    let diags = check("if (length(x == y)) print(\"bad\")\nok <- length(x) == y\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        1,
        "only the comparison nested directly under length should fire: {diags:?}"
    );
}

#[test]
fn comparison_inside_selected_scalar_calls_is_diagnosed() {
    let diags = check("length(x > 0)\nnchar(x == y)\nabs(x != y)\nsum(x > 0)\nlength(x) > 0\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        2,
        "length and nchar should fire under RY093: {diags:?}"
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY100")
            .count(),
        1,
        "abs should fire under RY100, but sum and an outer comparison should not: {diags:?}"
    );
}

#[test]
fn comparison_directly_inside_numeric_math_is_diagnosed() {
    let diags = check(
        "abs(x > y)\nabs(x) > y\nsqrt(a == b)\nsum(x > 0)\nlog(x, base = 2)\nabs(x %in% y)\nabs((x > y))\n",
    );
    let math: Vec<_> = diags
        .iter()
        .filter(|diagnostic| diagnostic.code == "RY100")
        .collect();
    assert_eq!(
        math.len(),
        3,
        "only direct ordinary comparisons, including extra parentheses, should fire: {diags:?}"
    );
    assert!(
        math.iter()
            .all(|diagnostic| diagnostic.severity == Severity::Warning),
        "RY100 must be a warning: {diags:?}"
    );
}

#[test]
fn sign_comparison_is_an_allowed_indicator_idiom() {
    let diags = check("sign(x <= y)\nabs(x <= y)\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY100")
            .count(),
        1,
        "sign() must be allowed, while abs() remains diagnosed: {diags:?}"
    );
}

#[test]
fn comparison_inside_call_is_diagnosed_through_short_circuit_operators() {
    let diags = check(
        "q <- TRUE\nx <- 1L\ny <- 2L\nz <- TRUE\nif (length(x == y) || q) x\nstopifnot(length(x == y) && z)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        2,
        "both short-circuit operands must retain call diagnostics: {diags:?}"
    );
}

#[test]
fn negated_comparison_binds_loosely_and_stays_silent() {
    // R parses `!x == y` as `!(x == y)` (unary `!` binds looser than
    // comparison), so the idiomatic `!length(x) == 1` guard is correct
    // code. RY095 wrongly assumed C precedence and is retired.
    let diags =
        check("x <- c(1, 2)\nif (!length(x) == 1) x <- 1\nflag <- !\"a\" == \"b\"\n!(1L == 2L)\n");
    assert!(
        diags.is_empty(),
        "negated comparisons are valid R and must stay silent: {diags:?}"
    );
}

#[test]
fn hasarg_requires_a_formal_of_the_lexically_enclosing_function() {
    let diags = check(
        "good <- function(value) hasArg(value)\ndots_ok <- function(actual, ...) hasArg(missing)\nidiom_ok <- function(object, ...) if (hasArg(thresh)) list(...)$thresh else 0\nstring_bad <- function(actual) hasArg(\"missing\")\nbad <- function(actual) hasArg(missing)\nhasArg(top_level)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY096")
            .count(),
        2,
        "non-formals in dots-less functions should fire; formals, dots functions, and top-level calls stay silent: {diags:?}"
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "hasArg captures names and must not create unbound-name diagnostics: {diags:?}"
    );
}

#[test]
fn printf_family_literal_arity_is_checked() {
    let diags = check(
        "gettextf(\"select %s then %s\", \"first\")\nsprintf(\"value=%s %%\", \"ok\")\nsprintf(dynamic_format, value)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY094")
            .count(),
        1,
        "only a proven literal format shortage should fire: {diags:?}"
    );
}
