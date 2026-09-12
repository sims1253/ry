use super::*;

// R's function-mode call-head lookup (#381/#384): a bare symbol in call
// position searches every frame for a *function* binding and skips
// non-function values. A symbol-head RY070 may therefore fire only when
// no function of that name is reachable anywhere and the outward search
// path is closed. Value reads and value-expression heads (qualified
// `pkg::dataset()`, literal heads) keep their separate, unchanged
// errors.

#[test]
fn outer_function_behind_function_local_value() {
    // R: `f <- function() "outer"; g <- function(){ f <- 5; f() }; g()`
    // returns "outer" -- the local value never masks the outward
    // function at a call head.
    let diagnostics = check("f <- function() \"outer\"\ng <- function() { f <- 5; f() }\ng()\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "function-mode lookup must skip the local non-function: {diagnostics:?}"
    );
}

#[test]
fn sequential_top_level_rebind_over_callable_keeps_error() {
    // Top-level source order is real: at the earlier call site the
    // binding is still the integer, so R errors. The whole-project
    // callable inventory must not hide a same-scope sequential error.
    let diagnostics = check("x <- 1L\nx()\nx <- S7::new_class(\"x\")\n");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "a top-level sequential value binding is the binding at call time: {diagnostics:?}"
    );
}

#[test]
fn uncertain_search_path_stays_silent() {
    // `library()` of a package with no stub opens the search path: a
    // function named `h` could be attached there, so absence is not
    // proof and RY070 must not fire.
    let diagnostics = check("h <- 5\nlibrary(notastubbedpkg)\nh()\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "an open search path hides functions; silence is required: {diagnostics:?}"
    );
}

#[test]
fn standalone_dataset_call_keeps_error() {
    // `trees()` with no function anywhere: R errors
    // `could not find function "trees"` -- the attached dataset is
    // invisible at call heads. The true no-function case stays an error.
    let diagnostics = check("trees()\n");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "a bare dataset name with no function anywhere is a real call error: {diagnostics:?}"
    );
}

#[test]
fn local_value_with_no_function_keeps_error() {
    let diagnostics = check("h <- 5\nh()\n");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "no function `h` exists anywhere; R errors could not find function: {diagnostics:?}"
    );
}

#[test]
fn qualified_dataset_call_keeps_error() {
    // `::` is value lookup: `datasets::trees()` reaches the data frame
    // and R errors `attempt to apply non-function`.
    let diagnostics = check("datasets::trees()\n");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "RY070" && d.message.contains("datasets::trees")),
        "qualified dataset calls keep their value-head error: {diagnostics:?}"
    );
}

#[test]
fn qualified_function_call_resolves() {
    let diagnostics = check("m <- stats::median(c(1, 2))\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "a qualified function call must resolve its signature: {diagnostics:?}"
    );
}

#[test]
fn local_function_masks_dataset_name() {
    let diagnostics = check("trees <- function() 1L\ntrees()\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "a lexical function wins over the same-named dataset stub: {diagnostics:?}"
    );
}

#[test]
fn literal_value_head_keeps_error() {
    let diagnostics = check("a <- 42()\n");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "literal value heads keep their not-callable error: {diagnostics:?}"
    );
}

#[test]
fn paren_value_head_without_outward_function_keeps_error() {
    // The parser lowers `(x)` to `x`, so this takes the symbol path; R
    // errors `attempt to apply non-function`. With no outward function
    // the symbol path reaches the same error, so the diagnostic holds.
    let diagnostics = check("x <- 5\n(x)()\n");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "paren-wrapped value heads keep the error while none is masked: {diagnostics:?}"
    );
}

#[test]
fn paren_value_head_with_outward_function_is_a_known_fn() {
    // Boundary: R errors `attempt to apply non-function` here because
    // `(x)` is a value head, but the parser lowers the parens away, so
    // the call takes the symbol path and the outward function wins. This
    // documents the pre-existing false negative; it must not regress
    // into an RY070 either.
    let diagnostics = check("x <- function() \"fn\"\ng <- function() { x <- 5; (x)() }\ng()\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "paren-wrapped heads follow the symbol path today: {diagnostics:?}"
    );
}

#[test]
fn same_frame_sequential_callable_rebind_keeps_error() {
    // A constructor bound and then overwritten inside the SAME function
    // frame: at the call the binding is the value, so R errors. Nested
    // assignments never enter the project callable inventory, so this
    // path is unaffected by the retained carveout.
    let diagnostics =
        check("g <- function() { Gen <- S7::new_class(\"Gen\"); Gen <- 5; Gen() }\ng()\n");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "same-frame sequential rebinding leaves a value bound at the call: {diagnostics:?}"
    );
}

#[test]
fn top_level_overwrite_before_frame_call_keeps_error() {
    // The last top-level assignment wins in the callable inventory, so
    // an overwritten generator no longer counts as call-head evidence.
    let diagnostics = check(
        "Gen <- S7::new_class(\"Gen\")\nGen <- 5\ng <- function() { Gen <- 5; Gen() }\ng()\n",
    );
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "an overwritten top-level generator is not outward evidence: {diagnostics:?}"
    );
}

#[test]
fn constructor_in_outer_function_frame_remains_callable() {
    let diagnostics = check(
        "outer <- function() { Gen <- S7::new_class(\"Gen\"); inner <- function() { Gen <- 5; Gen() }; inner() }\nouter()\n",
    );
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "{diagnostics:?}"
    );
}

#[test]
fn unknown_outward_binding_can_supply_a_function() {
    let diagnostics = check("Gen <- get(\"x\")\ng <- function() { Gen <- 5; Gen() }\ng()\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "{diagnostics:?}"
    );
}

#[test]
fn outward_parameter_and_multiple_frames_keep_function_uncertainty() {
    for source in [
        "outer <- function(f = 1L) { inner <- function() { f <- 2L; f() }; inner() }",
        "outer <- function() { f <- get(\"callback\"); middle <- function() { f <- 1L; inner <- function() { f <- 2L; f() }; inner() }; middle() }",
        "f <- if (getOption(\"choice\")) function() 1L else 2L; inner <- function() { f <- 3L; f() }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY070"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn known_non_function_outward_frames_keep_the_call_error() {
    let diagnostics = check(
        "outer <- function() { f <- 1L; inner <- function() { f <- 2L; f() }; inner() }; outer()",
    );
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "{diagnostics:?}"
    );
}

#[test]
fn open_search_path_overrides_callable_carveout() {
    // Uncertainty is decided before the callable-inventory carveout: an
    // open search path can hide a function of the name, so no RY070 may
    // fire even where the carveout would otherwise claim a proven error.
    let diagnostics = check("x <- 1L\nlibrary(notastubbedpkg)\nx()\nx <- S7::new_class(\"x\")\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "uncertainty must silence the call-head error before the carveout: {diagnostics:?}"
    );
}

#[test]
fn unknown_data_mask_stays_silent() {
    // Inside an unenumerable data mask a function binding of the name
    // could exist in the mask, so the value alone is not proof.
    let diagnostics = check("h <- 5\nd <- get(\"df\")\nwith(d, h())\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "an unknown data mask can hold a function binding: {diagnostics:?}"
    );
}

#[test]
fn open_search_path_silences_bare_dataset_head() {
    // First-site counterpart of `uncertain_search_path_stays_silent`:
    // the bare typeshed-value stage (#384's site) must honor `Uncertain`
    // too, not just the lexical-value stage. With the search path open,
    // a function named `trees` could be attached, so the dataset stub
    // alone is not proof of a call error.
    let diagnostics = check("library(notastubbedpkg)\ntrees()\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "the bare dataset-value stage must stay silent under uncertainty: {diagnostics:?}"
    );
}

#[test]
fn future_top_level_function_does_not_hide_an_earlier_failed_call() {
    for source in [
        "x <- 1L; x(); x <- function() 2L",
        "x <- function() 2L; x <- 1L; x()",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY070"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn outward_base_functions_and_deferred_bodies_keep_function_lookup() {
    for source in [
        "mean <- 1L; mean(c(1, 2)); mean <- function(x) x",
        "g <- function() { x <- 1L; x() }; x <- function() 2L; g()",
        "x <- 1L; library(notastubbedpkg); x(); x <- function() 2L",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY070"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn deferred_function_lookup_keeps_return_type_information() {
    let diagnostics =
        check("x <- function() 2L; g <- function() { x <- 1L; x() }; result <- g(); result$field");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY061"),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|d| d.code != "RY070"),
        "{diagnostics:?}"
    );
}

#[test]
fn eager_callbacks_use_current_outward_bindings() {
    for source in [
        "x <- function() 2L; lapply(1:3, function(i) { x <- 1L; x() })",
        "Gen <- S7::new_class('Gen'); lapply(1:3, function(i) { Gen <- 1L; Gen() })",
        "x <- get('callback'); lapply(1:3, function(i) { x <- 1L; x() })",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY070"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics = check("lapply(1:3, function(i) { x <- 1L; x() }); x <- function() 2L");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "{diagnostics:?}"
    );
}

#[test]
fn local_foreach_and_data_masks_keep_outward_callables() {
    for source in [
        "x <- function() 1L; local({ x <- 1L; x() })",
        "x <- function() 1L; base::local(expr = { x <- 1L; x() })",
        "x <- function() 1L; foreach(i = 1:3) %dopar% { x <- 1L; x() }",
        "x <- function() 1L; with(data.frame(x = 1L), x())",
        "x <- function() 1L; local({ x <- 1L; x() }, envir = new.env())",
        "library(foreach); x <- function() 1L; local({ x <- 1L; x() })",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY070"),
            "{source}: {diagnostics:?}"
        );
    }
    for source in [
        "x <- function() 1L; foreach(i = 1:3) %do% { x <- 1L; x() }",
        "local({ missing_callable_xyz <- 1L; missing_callable_xyz() })",
        "foreach(i = 1:3) %do% { missing_callable_xyz <- 1L; missing_callable_xyz() }",
        "x <- function() 1L; local({ x <- 1L; x() }, envir = environment())",
        "local <- function(expr) expr; x <- function() 1L; local({ x <- 1L; x() })",
        "x <- function() 1L; evalq({ x <- 1L; x() })",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY070"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics = check("x <- list(field = 1L); local({ x <- 1L }); x$field");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY061"),
        "{diagnostics:?}"
    );
}

#[test]
fn uncertain_local_calls_preserve_argument_diagnostics() {
    for source in [
        "local <- 1L; local({ missing_xyz; list(\"field\" <- 1L) })",
        "library(foreach); local({ missing_xyz; list(\"field\" <- 1L) })",
    ] {
        let diagnostics = check(source);
        for code in ["RY010", "RY102"] {
            assert!(
                diagnostics.iter().any(|d| d.code == code),
                "{source}: {diagnostics:?}"
            );
        }
    }
}

#[test]
fn uncertain_local_calls_join_possible_caller_writes() {
    for source in [
        "library(dplyr); x <- function() 1L; local({ x <- 1L }); x()",
        "local <- 1L; x <- function() 1L; local({ x <- 1L }); x()",
        "f <- function(local) { x <- 1L; local({ x <- list(field = 1L) }); x$field }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "RY070" && d.code != "RY061"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics =
        check("local <- function(expr) expr; x <- function() 1L; local({ x <- 1L }); x()");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY070"),
        "{diagnostics:?}"
    );
}

#[test]
fn local_nonlocal_writes_invalidate_caller_facts() {
    let diagnostics = check("x <- 1L; local({ x <<- list(field = 1L) }); x$field");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY061"),
        "{diagnostics:?}"
    );
    let diagnostics = check("x <- 1L; local({ x <- list(field = 1L) }); x$field");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY061"),
        "{diagnostics:?}"
    );
}
