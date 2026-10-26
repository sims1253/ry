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
        // No `...` in the real signature: partial names bind `error`.
        "expect_snapshot(1 + \"\", err = TRUE)\n",
        "expect_error(1 + \"\", cl = \"my_err\")\n",
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
        // A literal non-function handler catches nothing in R.
        "tryCatch(1 + \"\", error = NULL)\n",
        "tryCatch(1 + \"\", error = NA)\n",
        "tryCatch(1 + \"\", error = \"x\")\n",
        "tryCatch(1 + \"\", error = 1)\n",
        "tryCatch(1 + \"\", error = TRUE)\n",
        // Current testthat `check_bool(error)` rejects numerics
        // before evaluating the probe, so none of these assert.
        "expect_snapshot(1 + \"\", error = 0)\n",
        "expect_snapshot(1 + \"\", error = 1)\n",
        "expect_snapshot(1 + \"\", error = NaN)\n",
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
    // Refusal direction: a different attached package is not testthat.
    let diagnostics = check_with("expect_error(1 + \"\")\n", |c| {
        c.set_bare_loaded(std::collections::HashSet::from(["dplyr".to_string()]));
    });
    assert_eq!(
        arith_from(diagnostics)
            .into_iter()
            .map(|(_, c)| c)
            .collect::<Vec<_>>(),
        [Confidence::Medium]
    );
}

#[test]
fn no_error_assertions_and_deferred_bodies_keep_full_strength() {
    for source in [
        // `regexp = NA` asserts that NO error/warning occurs.
        "expect_error(1 + \"\", regexp = NA)\n",
        "expect_error(1 + \"\", NA)\n",
        "expect_warning(1 + \"\", regexp = NA)\n",
        // The probe catches creating the closure, not calling it later.
        "testthat::expect_error({ f <- function() { 1 + \"\" }; stop(\"x\") })\n",
    ] {
        assert_eq!(
            confidences(source),
            [Confidence::Medium],
            "genuine defect must keep full strength: {source}"
        );
    }
    // A probe inside the deferred body still demotes its own probe.
    assert_eq!(
        confidences(
            "testthat::expect_error({ f <- function() expect_error(1 + \"\"); stop(\"x\") })\n"
        ),
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
