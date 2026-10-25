use super::*;

fn codes(src: &str) -> Vec<&'static str> {
    check(src).into_iter().map(|d| d.code).collect()
}

fn warns_ry122(src: &str) -> bool {
    codes(src).contains(&"RY122")
}

#[test]
fn epoxy_negative_branch_warns() {
    assert!(warns_ry122(
        "page <- if (!requireNamespace(\"bslib\", quietly = TRUE)) bslib::page_fluid else shiny::fluidPage\n"
    ));
}

#[test]
fn negative_branch_call_and_triple_colon_warn() {
    assert!(warns_ry122(
        "if (!requireNamespace(\"pkgx\", quietly = TRUE)) { y <- pkgx::thing(1L) }\n"
    ));
    assert!(warns_ry122(
        "if (!requireNamespace(\"pkgx\", quietly = TRUE)) { y <- pkgx:::thing }\n"
    ));
}

#[test]
fn base_qualified_require_namespace_warns() {
    assert!(warns_ry122(
        "if (!base::requireNamespace(\"pkgx\", quietly = TRUE)) pkgx::thing else 1L\n"
    ));
}

#[test]
fn positive_guard_is_silent() {
    assert!(!warns_ry122(
        "page <- if (requireNamespace(\"bslib\", quietly = TRUE)) bslib::page_fluid else shiny::fluidPage\n"
    ));
}

#[test]
fn else_of_positive_guard_warns() {
    assert!(warns_ry122(
        "page <- if (requireNamespace(\"bslib\", quietly = TRUE)) shiny::fluidPage else bslib::page_fluid\n"
    ));
}

#[test]
fn other_package_in_failed_branch_is_silent() {
    assert!(!warns_ry122(
        "page <- if (!requireNamespace(\"bslib\", quietly = TRUE)) shiny::fluidPage else bslib::page_fluid\n"
    ));
}

#[test]
fn masked_require_namespace_is_silent() {
    assert!(!warns_ry122(
        "requireNamespace <- function(...) TRUE\nif (!requireNamespace(\"pkgx\", quietly = TRUE)) pkgx::thing else 1L\n"
    ));
}

#[test]
fn dynamic_package_name_is_silent() {
    assert!(!warns_ry122(
        "f <- function(p) if (!requireNamespace(p, quietly = TRUE)) pkgx::thing else 1L\n"
    ));
    assert!(!warns_ry122(
        "if (!requireNamespace(paste0(\"pkg\", \"x\"), quietly = TRUE)) pkgx::thing else 1L\n"
    ));
}

#[test]
fn without_quietly_true_is_silent() {
    // Without `quietly = TRUE`, base `requireNamespace` itself errors on
    // failure, so the FALSE branch never runs.
    assert!(!warns_ry122(
        "if (!requireNamespace(\"pkgx\")) pkgx::thing else 1L\n"
    ));
}

#[test]
fn trycatch_probe_is_silent() {
    assert!(!warns_ry122(
        "page <- if (!requireNamespace(\"pkgx\", quietly = TRUE)) tryCatch(pkgx::thing, error = function(e) 1L) else 2L\n"
    ));
}

#[test]
fn unknown_call_invalidates_the_fact() {
    assert!(!warns_ry122(
        "if (!requireNamespace(\"pkgx\", quietly = TRUE)) { setup_packages(); y <- pkgx::thing } else { y <- 1L }\n"
    ));
}

#[test]
fn fact_does_not_leak_past_the_if() {
    assert!(!warns_ry122(
        "if (!requireNamespace(\"pkgx\", quietly = TRUE)) { y <- 1L } else { y <- 2L }\nz <- pkgx::thing\n"
    ));
}

#[test]
fn closure_body_does_not_inherit_the_fact() {
    // A closure defined on the failed branch may run later, after the
    // namespace loads, so its body is not dominated by the failure.
    assert!(!warns_ry122(
        "if (!requireNamespace(\"pkgx\", quietly = TRUE)) { g <- function() pkgx::thing } else { g <- function() 1L }\n"
    ));
}

#[test]
fn nested_positive_guard_clears_the_fact() {
    assert!(!warns_ry122(
        "if (!requireNamespace(\"pkgx\", quietly = TRUE)) { if (requireNamespace(\"pkgx\", quietly = TRUE)) pkgx::thing else 1L } else 2L\n"
    ));
}
