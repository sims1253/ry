use super::*;

/// RY040 findings as `(span start, confidence)`, in source order.
fn arith(source: &str) -> Vec<(usize, Confidence)> {
    arith_from(check(source))
}

fn arith_from(mut diagnostics: Vec<Diagnostic>) -> Vec<(usize, Confidence)> {
    diagnostics.retain(|d| d.code == "RY040");
    diagnostics.sort_by_key(|d| d.span.start);
    diagnostics
        .iter()
        .map(|d| (d.span.start, d.confidence))
        .collect()
}

fn confidences(source: &str) -> Vec<Confidence> {
    arith(source).into_iter().map(|(_, c)| c).collect()
}

#[test]
fn recognized_error_wrappers_demote_the_probe_one_tier() {
    for source in [
        "expect_error(1 + \"\")\n",
        "testthat::expect_error(1 + \"\")\n",
        "expect_warning(1 + \"\")\n",
        "testthat::expect_warning(object = 1 + \"\")\n",
        "expect_error(object = 1 + \"\")\n",
        "expect_snapshot(error = TRUE, { 1 + \"\" })\n",
        "expect_snapshot(1 + \"\", error = TRUE)\n",
        "testthat::expect_snapshot(x = { 1 + \"\" }, error = TRUE)\n",
        "tryCatch(1 + \"\", error = function(e) NULL)\n",
        "base::tryCatch(expr = 1 + \"\", error = function(e) NULL)\n",
        "tryCatch(error = function(e) NULL, 1 + \"\")\n",
        "try(1 + \"\")\n",
        "base::try(expr = 1 + \"\", silent = TRUE)\n",
    ] {
        assert_eq!(
            confidences(source),
            [Confidence::Low],
            "probe must be demoted: {source}"
        );
    }
}

#[test]
fn identical_code_outside_a_wrapper_is_unchanged() {
    assert_eq!(confidences("1 + \"\"\n"), [Confidence::Medium]);
    // Severity is not part of the down-weight: the finding stays an
    // error that `--min-confidence` can filter.
    let inside = check("expect_error(1 + \"\")\n");
    let hit = inside.iter().find(|d| d.code == "RY040").unwrap();
    assert_eq!(hit.severity, Severity::Error);
}

#[test]
fn non_error_wrappers_and_missing_handlers_stay_full_strength() {
    for source in [
        "expect_snapshot(1 + \"\")\n",
        "expect_snapshot(1 + \"\", error = FALSE)\n",
        "ea <- TRUE\nexpect_snapshot(1 + \"\", error = ea)\n",
        "tryCatch(1 + \"\", warning = function(w) NULL)\n",
        "tryCatch(1 + \"\", finally = NULL)\n",
        "expect_equal(1 + \"\", 1)\n",
        "other::expect_error(1 + \"\")\n",
    ] {
        assert_eq!(
            confidences(source),
            [Confidence::Medium],
            "no error is asserted by construction: {source}"
        );
    }
}

#[test]
fn masked_wrappers_stay_full_strength() {
    for source in [
        "expect_error <- function(object) object\nexpect_error(1 + \"\")\n",
        "tryCatch <- function(expr, ...) expr\ntryCatch(1 + \"\", error = function(e) NULL)\n",
        "try <- function(expr) expr\ntry(1 + \"\")\n",
        "f <- function(expect_error) expect_error(1 + \"\")\n",
        "f <- function() { expect_snapshot <- function(x, error = FALSE) x; expect_snapshot(1 + \"\", error = TRUE) }\n",
    ] {
        assert_eq!(
            confidences(source),
            [Confidence::Medium],
            "a local or project definition masks the wrapper: {source}"
        );
    }
    // Qualification bypasses the local mask and demotes again.
    assert_eq!(
        confidences("expect_error <- function(object) object\ntestthat::expect_error(1 + \"\")\n"),
        [Confidence::Low]
    );
}

#[test]
fn non_probe_arguments_keep_full_strength() {
    // The regexp argument is ordinary caller code, not the probe.
    assert_eq!(
        confidences("expect_error(object = 1 + \"\", regexp = 2 + \"\")\n"),
        [Confidence::Low, Confidence::Medium]
    );
    // A `finally` expression runs on every path; its error is not caught.
    assert_eq!(
        confidences("tryCatch(1 + \"\", error = function(e) NULL, finally = 2 + \"\")\n"),
        [Confidence::Low, Confidence::Medium]
    );
    // Code after the wrapper is unaffected.
    assert_eq!(
        confidences("expect_error(1 + \"\")\n2 + \"\"\n"),
        [Confidence::Low, Confidence::Medium]
    );
}

#[test]
fn attached_testthat_still_recognizes_bare_wrappers() {
    let diagnostics = check_with("expect_error(1 + \"\")\n", |c| {
        c.set_bare_loaded(std::collections::HashSet::from(["testthat".to_string()]));
    });
    assert_eq!(
        arith_from(diagnostics)
            .into_iter()
            .map(|(_, c)| c)
            .collect::<Vec<_>>(),
        [Confidence::Low]
    );
}

#[test]
fn nested_wrappers_demote_only_once() {
    assert_eq!(
        confidences("expect_error(tryCatch(1 + \"\", error = function(e) NULL))\n"),
        [Confidence::Low]
    );
    // A block probe demotes every diagnostic inside it, including one
    // that precedes the asserted error.
    assert_eq!(
        confidences("expect_snapshot(error = TRUE, {\n  x <- 1L\n  x + \"\"\n})\n"),
        [Confidence::Low]
    );
}
