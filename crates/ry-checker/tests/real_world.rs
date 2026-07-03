//! Real-world snapshot tests (PLAN Phase 7 item 3).
//!
//! Runs the checker against a small set of vendored "realistic" R
//! snippets (each modeled after real CRAN/base R code patterns) and
//! ASSERTS the exact diagnostic list via `insta` snapshots. Each
//! diagnostic in a snapshot is triaged in a comment below the snippet:
//! true positive or known-limitation.
//!
//! A snapshot failing is the false-positive alarm the old assertion-free
//! "baseline report" printers pretended to be: if the diagnostic list
//! shifts, review the diff, update the snapshot deliberately, and record
//! why. Run `cargo insta review` to accept intentional changes.

use ry_checker::Checker;
use ry_core::RParser;

/// Check `src` and return the sorted list of `(code, message)` tuples so
/// the snapshot is stable across HashMap iteration order.
fn check_sorted(name: &str, src: &str) -> Vec<(String, String)> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse(name, src).expect("parse");
    let mut c = Checker::new(name);
    c.check(&file);
    let mut diags: Vec<(String, String)> = c
        .take_diagnostics()
        .into_iter()
        .map(|d| (d.code.to_string(), d.message))
        .collect();
    diags.sort();
    diags
}

// ---- Snippet: base_mean_default ----
// Triage: (clean) -- no diagnostics expected. `mean` redefined with a
// default arg; the body's `is.na(x)` on the opaque param `x` stays
// quiet (opaque condition), `any(...)` resolves, `sum(x)/length(x)` is
// fine. A future improvement: flag the rebinding of a base function.
#[test]
fn snapshot_base_mean_default() {
    let src = r#"mean <- function(x, trim = 0, na.rm = FALSE) {
  if (!na.rm && any(is.na(x))) return(NA_real_)
  sum(x) / length(x)
}
"#;
    insta::assert_yaml_snapshot!(
        "snapshot_base_mean_default",
        check_sorted("base_mean_default", src)
    );
}

// ---- Snippet: tidyverse_pipe ----
// Triage: (clean). magrittr pipe desugaring: c(1,2,3) %>% mean() %>% round(2).
#[test]
fn snapshot_tidyverse_pipe() {
    let src = r#"library(magrittr)
result <- c(1, 2, 3) %>%
  mean() %>%
  round(2)
"#;
    insta::assert_yaml_snapshot!(
        "snapshot_tidyverse_pipe",
        check_sorted("tidyverse_pipe", src)
    );
}

// ---- Snippet: mtcars_dataset ----
// Triage: (clean). `mtcars` resolves via the typeshed dataset table to a
// list-typed value (no RY010).
#[test]
fn snapshot_mtcars_dataset() {
    let src = r#"df <- mtcars
head(df)
"#;
    insta::assert_yaml_snapshot!(
        "snapshot_mtcars_dataset",
        check_sorted("mtcars_dataset", src)
    );
}

// ---- Snippet: pipe_subset_nse ----
// Triage: (clean). NSE: subset(mtcars, cyl == 4) -- `cyl` resolves via the
// data.frame's column schema (the checker augments the inference scope).
#[test]
fn snapshot_pipe_subset_nse() {
    let src = r#"library(magrittr)
result <- mtcars %>% subset(cyl == 4)
"#;
    insta::assert_yaml_snapshot!(
        "snapshot_pipe_subset_nse",
        check_sorted("pipe_subset_nse", src)
    );
}

// ---- Snippet: s3_dispatch ----
// Triage: (clean). print.myclass is a real S3 method (first param `x`);
// structure(...) attaches class "myclass"; print(x) dispatches. No
// diagnostics expected.
#[test]
fn snapshot_s3_dispatch() {
    let src = r#"print.myclass <- function(x, ...) {
  cat("My class:", x$name, "\n")
  invisible(x)
}
x <- structure(list(name = "foo"), class = "myclass")
print(x)
"#;
    insta::assert_yaml_snapshot!("snapshot_s3_dispatch", check_sorted("s3_dispatch", src));
}

// ---- Snippet: na_propagation ----
// Triage: (clean). c(1, 2, NA, 4) arithmetic and mean() with na.rm.
#[test]
fn snapshot_na_propagation() {
    let src = r#"x <- c(1, 2, NA, 4)
y <- x + 10
z <- mean(x, na.rm = TRUE)
"#;
    insta::assert_yaml_snapshot!(
        "snapshot_na_propagation",
        check_sorted("na_propagation", src)
    );
}

// ---- Snippet: coercion_ladder ----
// Triage: (clean). c() coercion and an if-expr join.
#[test]
fn snapshot_coercion_ladder() {
    let src = r#"x <- c(1L, 2L, 3L)
y <- c(x, 4.5)
z <- c(y, "end")
w <- if (length(z) > 0) z else NA
"#;
    insta::assert_yaml_snapshot!(
        "snapshot_coercion_ladder",
        check_sorted("coercion_ladder", src)
    );
}

// ---- Snippet: loop_with_accumulator ----
// Triage: (clean). for-loop accumulator pattern.
#[test]
fn snapshot_loop_with_accumulator() {
    let src = r#"total <- 0
for (i in 1:100) {
  if (i %% 2 == 0) {
    total <- total + i
  }
}
"#;
    insta::assert_yaml_snapshot!(
        "snapshot_loop_with_accumulator",
        check_sorted("loop_with_accumulator", src)
    );
}

// ---- Snippet: vectorized_op ----
// Triage: (clean). vectorized arithmetic + which().
#[test]
fn snapshot_vectorized_op() {
    let src = r#"x <- 1:10
y <- x * 2 + 1
ok <- y > 5
sel <- which(ok)
"#;
    insta::assert_yaml_snapshot!("snapshot_vectorized_op", check_sorted("vectorized_op", src));
}
