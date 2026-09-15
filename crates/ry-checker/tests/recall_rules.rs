//! Recall rules.
//!
//! Four rules were sketched during the Posit corpus audit response, aimed at
//! the false-negative half of the corpus audit. Two ship whole, two ship in
//! half, and one does not ship at all.
//!
//! | rule name | code | shape |
//! |---|---|---|
//! | `named-list-element-arrow` | `RY102` | `list("a" <- 1)` |
//! | `class-equality` | `RY103` | `if (class(x) == "y")` |
//! | `constant-condition` (b) | `RY105` | `length(sum(v)) > 0` |
//! | `constant-condition` (a) | `RY107` | `any(v) == 0`, see below |
//! | `not-before-comparison` | — | **not shipped**, see the module test |
//!
//! `not-before-comparison` rests on the original claim that "`!` binds
//! tighter, so `!x >= y` is `(!x) >= y`". R parses it the other way:
//! `quote(!x == y)` is a call to `!` whose argument is `x == y`, because
//! negation binds *looser* than comparison. That is the same wrong
//! precedence model that retired `RY095`, and
//! `testdata/ry095_ry096_real_shapes.R` exists to pin it.
//!
//! `constant-condition`'s first half (`any(v) == 0`, glue `R/utils.R:32`)
//! was originally dropped alongside it. The original sketch justifies the
//! rule with "is always FALSE", which is not true: `any()` returns a
//! logical, and `FALSE == 0` is `TRUE`. glue's line is a real bug — the
//! author meant `any(lengths == 0)` — and the shape seemed indistinguishable
//! from diffobj's legitimate `!all(diff(x)) == 1L`, already pinned as
//! must-stay-silent in the same regression fixture. RY107 separates the two
//! by *outcome* rather than shape: a comparison that preserves the scalar
//! logical's value (`== 1`, `> 0`, ...) is the diffobj idiom and stays
//! silent, while one that negates it (`== 0`, `!= 1`, ...) or is constant
//! (`> 1`, `< 0`, ...) computes something other than what the element-level
//! reading suggests and is reported.
//!
//! Every rule is asserted against the corpus reproduction committed at
//! `testdata/err_recall_rules_repro.R`, which 0.8.0 checked completely clean.

use ry_checker::Checker;
use ry_core::RParser;

/// Codes emitted by the single-file checker for `src`.
fn codes(src: &str) -> Vec<&'static str> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("recall.R", src).expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    checker
        .take_diagnostics()
        .into_iter()
        .map(|d| d.code)
        .collect()
}

/// `(code, 1-based line)` pairs emitted for `src`.
fn code_lines(src: &str) -> Vec<(&'static str, usize)> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("recall.R", src).expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    checker
        .take_diagnostics()
        .into_iter()
        .map(|d| (d.code, d.span.line + 1))
        .collect()
}

fn fires(src: &str, code: &str) -> bool {
    codes(src).contains(&code)
}

/// The committed reproduction of the audit's false negatives.
fn repro_source() -> String {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/err_recall_rules_repro.R");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

// ---------------------------------------------------------------------------
// RY102 — named-list-element-arrow
// ---------------------------------------------------------------------------

#[test]
fn ry102_fires_on_string_arrow_in_list() {
    // `names(list("a" <- 1, "b" = 2))` is `c("", "b")`: the element silently
    // loses its name and a stray top-level binding `a` is created.
    assert!(fires("l <- list(\"a\" <- 1, \"b\" = 2)\n", "RY102"));
}

#[test]
fn ry102_fires_for_every_container_in_the_family() {
    for container in ["list", "c", "data.frame", "structure"] {
        let src = format!("v <- {container}(x = 1, \"nm\" <- 2)\n");
        assert!(fires(&src, "RY102"), "{container} did not fire RY102");
    }
}

#[test]
fn ry102_fires_on_namespaced_container() {
    assert!(fires("v <- base::list(\"nm\" <- 2)\n", "RY102"));
}

#[test]
fn ry102_fires_on_right_assign() {
    // `2 -> nm` lowers to the same shape with the operands swapped, so it
    // takes the identifier path and needs the same corroboration: a named
    // sibling argument proving the call builds a named structure.
    assert!(fires("v <- list(a = 1, 2 -> nm)\n", "RY102"));
}

#[test]
fn ry102_stays_silent_on_bare_assign_and_append() {
    // `c(out, outn <- paste(...))` is a deliberate assign-and-append idiom
    // (Hmisc, knitr, nlme, xfun, DescTools, fitdistrplus). With no named
    // sibling there is no evidence the call builds a named structure, so
    // neither spelling of the assignment fires.
    assert!(!fires("v <- list(2 -> nm)\n", "RY102"));
    assert!(!fires("v <- list(nm <- 2)\n", "RY102"));
    assert!(!fires("v <- c(out, outn <- paste(1))\n", "RY102"));
}

#[test]
fn ry102_stays_silent_on_correct_naming() {
    assert!(!fires("l <- list(a = 1, b = 2)\n", "RY102"));
}

#[test]
fn ry102_stays_silent_on_equality_test_argument() {
    // `==` is a comparison, not an assignment; nothing loses a name.
    assert!(!fires("x <- 1\nl <- list(x == 1)\n", "RY102"));
}

#[test]
fn ry102_stays_silent_outside_the_container_family() {
    // Only the containers whose arguments become *named elements* are
    // affected. `local(x <- 1)` and friends are ordinary assignments.
    assert!(!fires("v <- local(x <- 1)\n", "RY102"));
    assert!(!fires("f <- function(a) a\nv <- f(x <- 1)\n", "RY102"));
}

#[test]
fn ry102_stays_silent_on_super_assignment() {
    // `<<-` inside a container is an explicit side effect, not a mistyped
    // `=`: nobody reaches for `<<-` when they meant to name an element.
    assert!(!fires("x <- 1\nv <- list(x <<- 2)\n", "RY102"));
}

#[test]
fn ry102_stays_silent_on_complex_lhs() {
    // `list(x[[1]] <- 2)` cannot be a mistyped `name = value`.
    assert!(!fires("x <- list(1)\nv <- list(x[[1]] <- 2)\n", "RY102"));
}

// ---------------------------------------------------------------------------
// RY103 — class-equality
// ---------------------------------------------------------------------------

#[test]
fn ry103_fires_in_if_condition() {
    assert!(fires(
        "f <- function(x) if (class(x) == \"data.frame\") 1 else 2\n",
        "RY103"
    ));
}

#[test]
fn ry103_fires_under_and_and() {
    assert!(fires(
        "f <- function(x, t) if (!is.null(t) && class(x) != t) 1\n",
        "RY103"
    ));
}

#[test]
fn ry103_fires_under_or_or() {
    assert!(fires(
        "f <- function(x) if (is.null(x) || class(x) == \"tbl\") 1\n",
        "RY103"
    ));
}

#[test]
fn ry103_fires_through_negation() {
    assert!(fires(
        "f <- function(x) if (!(class(x) == \"tbl\")) 1\n",
        "RY103"
    ));
}

#[test]
fn ry103_fires_in_while_condition() {
    assert!(fires(
        "f <- function(x) while (class(x) != \"tbl\") x <- unclass(x)\n",
        "RY103"
    ));
}

#[test]
fn ry103_fires_once_per_site() {
    // The `if` scan must not double-report a comparison that the `&&`
    // handler already covers.
    let src = "f <- function(x, t) if (!is.null(t) && class(x) != t) 1\n";
    let hits = codes(src).iter().filter(|c| **c == "RY103").count();
    assert_eq!(hits, 1, "expected exactly one RY103, got {hits}");
}

#[test]
fn ry103_stays_silent_for_inherits() {
    assert!(!fires(
        "f <- function(x) if (inherits(x, \"data.frame\")) 1\n",
        "RY103"
    ));
}

#[test]
fn ry103_stays_silent_for_class_index() {
    // `class(x)[1]` is explicitly length-1, which is the documented way to
    // write this comparison when `inherits()` is not wanted.
    assert!(!fires(
        "f <- function(x) if (class(x)[1] == \"tbl\") 1\n",
        "RY103"
    ));
}

#[test]
fn ry103_stays_silent_outside_a_condition() {
    // Vectorised use is exactly what `class()` returning a vector is for.
    assert!(!fires("f <- function(x) class(x) == \"tbl\"\n", "RY103"));
    assert!(!fires(
        "f <- function(x) any(class(x) == \"tbl\")\n",
        "RY103"
    ));
}

#[test]
fn ry103_stays_silent_for_elementwise_operators() {
    // `&` / `|` are vectorised; they do not require a length-1 operand.
    assert!(!fires(
        "f <- function(x) y <- (class(x) == \"tbl\") & TRUE\n",
        "RY103"
    ));
}

// ---------------------------------------------------------------------------
// RY105 — constant-length-comparison (`length(sum(v)) > 0`)
// ---------------------------------------------------------------------------

#[test]
fn ry105_fires_on_length_of_a_scalar_reduction() {
    // pak R/confirmation.R:42. `length(sum(...))` is 1 by construction, so
    // the guard is always TRUE.
    assert!(fires("if (length(sum(1L)) > 0) 1\n", "RY105"));
}

#[test]
fn ry105_fires_through_a_local_binding() {
    assert!(fires(
        "f <- function(s) {\n  n <- sum(is.na(s))\n  length(n) > 0\n}\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_for_a_classed_scalar_local() {
    // Storage length does not determine `length(x)` for an S3 object:
    // `length.<class>` may return any length.
    assert!(!fires(
        "length.ry_cleanup <- function(x) 0L\nx <- structure(1L, class = \"ry_cleanup\")\nif (length(x) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_for_a_classed_scalar_call() {
    // A scalar-returning typeshed generic may dispatch to a method that
    // returns a non-scalar vector.
    assert!(!fires(
        "sum.ry_cleanup <- function(x, ...) integer(0)\nx <- structure(1L, class = \"ry_cleanup\")\nif (length(sum(x)) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
    // A class constructor hidden inside the direct call carries the same
    // dispatch uncertainty as a classed local binding.
    assert!(!fires(
        "sum.ry_cleanup <- function(x, ...) integer(0)\nif (length(sum(structure(1L, class = \"ry_cleanup\"))) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_for_a_parameter_with_a_scalar_default() {
    assert!(!fires(
        "sum.ry_cleanup <- function(x, ...) integer(0)\nf <- function(x = 1L) if (length(sum(x)) > 0L) stop(\"unreachable\")\nf(structure(1L, class = \"ry_cleanup\"))\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_for_a_masked_operator_argument() {
    assert!(!fires(
        "`+` <- function(x, y) structure(1L, class = \"ry_cleanup\")\nsum.ry_cleanup <- function(x, ...) integer(0)\nif (length(sum(1L + 1L)) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_when_a_scalar_class_is_unknown() {
    // A dynamic class may add an S3 length method, so storage length is not
    // enough evidence for the zero-length comparison.
    assert!(!fires(
        "class_name <- \"ry_cleanup\"\nx <- structure(1L, class = class_name)\nif (length(x) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_when_length_is_masked() {
    assert!(!fires(
        "length <- function(x) 1L\nx <- 1L\nif (length(x) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_on_a_plain_vector() {
    assert!(!fires("f <- function(v) if (length(v) > 0) 1\n", "RY105"));
}

#[test]
fn ry105_retains_the_classless_scalar_warning() {
    assert!(fires(
        "x <- 1L\nif (length(x) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_on_a_parameter() {
    // A parameter's type comes from a default or a single call site; it is
    // not proof of the runtime length.
    assert!(!fires(
        "f <- function(n = 1) if (length(n) > 0) 1\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_against_a_non_constant_bound() {
    // `length(sum(v)) > k` is only constant when the bound is a literal.
    assert!(!fires(
        "f <- function(v, k) if (length(sum(v)) > k) 1\n",
        "RY105"
    ));
}

// ---------------------------------------------------------------------------
// RY105 — recycling constructors and projections keep length facts sound (#377)
// ---------------------------------------------------------------------------

#[test]
fn ry105_stays_silent_for_a_recycling_file_path() {
    // learnr R/run.R:290, pkgload R/load-code.R:65. `file.path()` recycles
    // its arguments, so a vector component propagates to the result; the
    // result is length 1 only when every argument provably is.
    assert!(!fires(
        "f <- function(src_root) if (length(file.path(src_root, \"x\")) > 0L) 1\n",
        "RY105"
    ));
    // ?file.path: any zero-length argument yields an empty character
    // vector (unlike paste, which recycles "" for zeros), so the
    // binding-form dead-guard claim must not fire either.
    assert!(!fires(
        "paths <- character(0)\np <- file.path(paths, \"x\")\nif (length(p) > 0L) 1\n",
        "RY105"
    ));
    // All-scalar-literal arguments do prove a length-1 result through a
    // local binding, and that dead guard is still reported.
    assert!(fires(
        "p <- file.path(\"a\", \"b\")\nif (length(p) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_for_seq_len_of_a_runtime_count() {
    // brulee R/augment.R:412. The result length of `seq_len(n)` is the
    // *value* of `n`, which a length-1 argument does not pin down.
    assert!(!fires(
        "f <- function(df) {\n  i <- seq_len(nrow(df))\n  if (length(i) > 0L) 1\n}\n",
        "RY105"
    ));
    assert!(!fires(
        "f <- function(n) if (length(seq_len(n)) > 0L) 1\n",
        "RY105"
    ));
    // A literal count yields the exact length: `seq_len(3)` is never empty,
    // but it is length 3, not 1, so no length-1 claim is made either way.
    assert!(!fires("if (length(seq_len(3)) > 0L) 1\n", "RY105"));
}

#[test]
fn ry105_projection_lengths_follow_r_null_padding() {
    // `[` pads out-of-bounds and empty selections for every vector mode,
    // lists included: `integer(0)[1]` is a length-1 NA and `list()[1]` is
    // `list(NULL)`, also length 1 — the semantics the 0.9.1 release screen
    // pinned on #377. The atomic binding form is therefore a true
    // positive: the guard really is always TRUE.
    assert!(fires(
        "x <- character(0)\nfirst <- x[1]\nif (length(first) > 0L) stop(\"unreachable\")\n",
        "RY105"
    ));
    // A list projection is also always length ≥ 1 at runtime, but the
    // local-binding admission requires an atomic mode, so ry stays silent
    // rather than claiming through a list-typed binding.
    assert!(!fires(
        "items <- list()\nfirst <- items[1]\nif (length(first) > 0L) 1\n",
        "RY105"
    ));
    assert!(!fires(
        "f <- function(items = list()) {\n  first <- items[1]\n  if (length(first) > 0L) 1\n}\n",
        "RY105"
    ));
}

#[test]
fn ry105_stays_silent_when_the_comparison_is_true_for_some_lengths() {
    // `length(x) == 1` on a provable scalar is a redundant assertion, not a
    // dead guard, and assertion helpers write it deliberately.
    assert!(!fires(
        "f <- function(v) if (length(sum(v)) == 1) 1\n",
        "RY105"
    ));
}

#[test]
fn ry105_normalizes_operand_order_for_constant_outcome() {
    // `0 > length(sum(v))` is `0 > 1`, which is FALSE — not TRUE. The
    // operator must be mirrored when the zero literal is on the left.
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse(
            "recall.R",
            "if (0 > length(sum(1L))) 1
",
        )
        .expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY105" && d.message.contains("FALSE")),
        "0 > length(1) is FALSE: {diags:?}"
    );

    // `0 < length(sum(v))` is `0 < 1`, which is TRUE.
    let file = parser
        .parse(
            "recall.R",
            "if (0 < length(sum(1L))) 1
",
        )
        .expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY105" && d.message.contains("TRUE")),
        "0 < length(1) is TRUE: {diags:?}"
    );
}

#[test]
fn ry105_stays_silent_for_unstubbed_reductions() {
    // `which.max` and `which.min` are not in the typeshed stubs, so
    // the checker has no evidence that they return length-1.
    // `which.max(numeric(0))` returns integer(0), which confirms the
    // stubs are right to omit them.
    assert!(!fires(
        "f <- function(v) if (length(which.max(v)) > 0) 1
",
        "RY105"
    ));
    assert!(!fires(
        "f <- function(v) if (length(which.min(v)) > 0) 1
",
        "RY105"
    ));
}

#[test]
fn ry105_respects_local_shadowing_of_scalar_reduction() {
    // A locally redefined `sum` that returns a vector must not trigger
    // RY105 on length(sum(x)) > 0. The base function's scalar property
    // only holds when the name is not shadowed.
    assert!(!fires(
        "f <- function(v) { sum <- function(x) c(x, x); if (length(sum(v)) > 0) 1 }
",
        "RY105"
    ));
    // Same for `any`.
    assert!(!fires(
        "f <- function(v) { any <- function(x) c(TRUE, FALSE); if (length(any(v)) > 0) 1 }
",
        "RY105"
    ));
}

// ---------------------------------------------------------------------------
// RY107 — any-all-scalar-comparison (`any(v) == 0`)
// ---------------------------------------------------------------------------

#[test]
fn ry107_fires_on_the_glue_parenthesization_bug() {
    // glue R/utils.R:32 (da9c73f). `any(lengths) == 0` computes
    // `!any(lengths)`, so with a zero length present the guard is FALSE
    // and the early return never runs. The same commit writes the intended
    // `any(lengths == 0)` twice in R/glue.R:139,191.
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse(
            "recall.R",
            "u <- function(x) {\n  lengths <- vapply(x, NROW, integer(1))\n  if (any(lengths) == 0) return(character())\n}\n",
        )
        .expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY107" && d.message.contains("any(lengths == 0)")),
        "expected RY107 suggesting the element-level rewrite: {diags:?}"
    );
}

#[test]
fn ry107_fires_on_negating_and_constant_outcomes_only() {
    // `== 0`/`!= 1` negate the scalar logical; `> 1`/`< 0` are constant
    // FALSE; `>= 0` is constant TRUE. All compute something other than
    // the element-level reading.
    for src in [
        "f <- function(x) if (any(x) == 0) 1\n",
        "f <- function(x) if (any(x) != 1) 1\n",
        "f <- function(x) if (all(x) <= 0) 1\n",
        "f <- function(x) if (all(x) < 1) 1\n",
        "f <- function(x) if (any(x) > 1) 1\n",
        "f <- function(x) if (any(x) < 0) 1\n",
        "f <- function(x) if (all(x) >= 0) 1\n",
        "f <- function(x) if (any(x) == 2) 1\n",
    ] {
        assert!(fires(src, "RY107"), "RY107 did not fire on {src:?}");
    }
}

#[test]
fn ry107_stays_silent_on_value_preserving_comparisons() {
    // `== 1`, `!= 0`, `> 0`, and `>= 1` compute exactly what the bare
    // call computes. diffobj's `!all(diff(x)) == 1L` is this family
    // written on purpose and is pinned as must-stay-silent in
    // testdata/ry095_ry096_real_shapes.R.
    for src in [
        "f <- function(x) any(x) == 1\n",
        "f <- function(x) any(x) != 0\n",
        "f <- function(x) any(x) > 0\n",
        "f <- function(x) all(x) >= 1\n",
        "f <- function(x) !all(diff(x)) == 1L\n",
    ] {
        assert!(!fires(src, "RY107"), "RY107 fired on the idiom {src:?}");
    }
}

#[test]
fn ry107_mirrors_the_suggestion_for_a_leading_literal() {
    // `0 == any(x)` must suggest `any(x == 0)`, not `any(x 0 ==)`.
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse("recall.R", "f <- function(x) if (0 == any(x)) 1\n")
        .expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY107" && d.message.contains("any(x == 0)")),
        "expected the mirrored rewrite: {diags:?}"
    );
}

#[test]
fn ry107_stays_silent_for_element_level_spellings() {
    // The comparison inside any()/all() is the intended form, and a call
    // with controls (`na.rm`) is not the single-argument shape whose
    // misplaced parenthesis this rule reconstructs.
    assert!(!fires("f <- function(x) if (any(x == 0)) 1\n", "RY107"));
    assert!(!fires("f <- function(x) if (all(x != 1)) 1\n", "RY107"));
    assert!(!fires(
        "f <- function(x) if (any(x, na.rm = TRUE) == 0) 1\n",
        "RY107"
    ));
}

#[test]
fn ry107_stays_silent_against_non_literal_operands() {
    // No dataflow: comparing any() with a name, a call, or a logical
    // literal is outside this rule.
    assert!(!fires("f <- function(x, k) if (any(x) == k) 1\n", "RY107"));
    assert!(!fires(
        "f <- function(x) if (any(x) == length(x)) 1\n",
        "RY107"
    ));
    assert!(!fires("f <- function(x) if (any(x) == FALSE) 1\n", "RY107"));
    assert!(!fires(
        "f <- function(x) if (any(x) == all(x)) 1\n",
        "RY107"
    ));
}

#[test]
fn ry107_respects_local_shadowing() {
    // A locally redefined `any` need not return a length-1 logical, so
    // the scalar premise does not hold for the shadowed callee.
    assert!(!fires(
        "f <- function(v) { any <- function(x) c(TRUE, FALSE); if (any(v) == 0) 1 }
",
        "RY107"
    ));
    // A base-qualified call keeps the premise.
    assert!(fires(
        "f <- function(v) if (base::any(v) == 0) 1\n",
        "RY107"
    ));
    // A foreign-qualified `any` is a different function.
    assert!(!fires(
        "f <- function(v) if (otherpkg::any(v) == 0) 1\n",
        "RY107"
    ));
}

#[test]
fn ry107_does_not_double_report_the_aggregate_neighbors() {
    // `length(any(x) == 0)` is RY093's span and `abs(any(x) == 0)` is
    // RY100's; RY107 yields when either already reported the comparison.
    // `length(any(1L)) > 0` is RY105's wrapped-length shape.
    for (src, neighbor) in [
        ("a <- length(any(x) == 0)\n", "RY093"),
        ("b <- abs(any(x) == 0)\n", "RY100"),
        ("c <- if (length(any(1L)) > 0) 1\n", "RY105"),
    ] {
        let emitted = codes(src);
        assert!(
            !emitted.contains(&"RY107"),
            "RY107 double-reported {src:?} alongside {neighbor}: {emitted:?}"
        );
        assert!(
            emitted.contains(&neighbor),
            "{neighbor} should own {src:?}: {emitted:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// `not-before-comparison` is deliberately NOT implemented
// ---------------------------------------------------------------------------

/// The audit-response sketch asked for a `not-before-comparison` rule on the premise that
/// "`!` binds tighter, so `!x >= y` is `(!x) >= y`". That premise is false.
/// R's `?Syntax` places unary `!` *below* the comparison operators, so
/// `!x >= y` parses as `!(x >= y)` — verified with
/// `Rscript -e 'as.list(quote(!x >= y))'`, which yields `` `!` `` applied to
/// `x >= y`.
///
/// This is exactly the model error that retired `RY095`
/// (`negation-comparison-precedence`) in 0.4.1, where every flagged corpus
/// site turned out to be correct code. No rule is shipped for this shape,
/// and this test pins the silence so the mistake is not made a third time.
#[test]
fn negation_before_comparison_is_not_diagnosed() {
    let sources = [
        "f <- function(x, y) if (!x >= y) 1\n",
        "f <- function(x, y) if (!x == y) 1\n",
        "f <- function(v) if (!nchar(v) > 3) 1\n",
    ];
    for src in sources {
        let emitted = codes(src);
        assert!(
            !emitted.contains(&"RY095"),
            "RY095 is retired and must never be reinstated: {src:?} emitted {emitted:?}"
        );
        // Nor may any of these codes stand in for it.
        for code in ["RY102", "RY103", "RY105", "RY107"] {
            assert!(
                !emitted.contains(&code),
                "{code} fired on a correctly-parsing negation: {src:?} emitted {emitted:?}"
            );
        }
    }
}

#[test]
fn ry095_stays_out_of_the_registry() {
    assert!(
        ry_checker::rules::find("RY095").is_none(),
        "RY095 is retired and its code must not be reused"
    );
    assert!(ry_checker::rules::find("negation-comparison-precedence").is_none());
}

// ---------------------------------------------------------------------------
// Corpus reproduction
// ---------------------------------------------------------------------------

/// Each shipped rule must fire on its line of the committed reproduction.
/// The acceptance criterion from that sketch: "Every new rule ships with
/// the corpus repro as a test fixture and fires on it."
#[test]
fn corpus_repro_fires_every_shipped_rule() {
    let src = repro_source();
    let hits = code_lines(&src);
    let expected = [
        ("RY102", 7, "pak R/pak-sitrep-data.R:41"),
        ("RY103", 11, "sparklyr R/worker_apply.R:522"),
        ("RY105", 31, "pak R/confirmation.R:42"),
        ("RY107", 22, "glue R/utils.R:32"),
    ];
    let mut missing = Vec::new();
    for (code, line, origin) in expected {
        if !hits.iter().any(|(c, l)| *c == code && *l == line) {
            missing.push(format!("{code} at line {line} ({origin})"));
        }
    }
    assert!(
        missing.is_empty(),
        "repro did not fire on: {missing:?}\nemitted: {hits:?}"
    );
}

/// The reproduction must gain *only* these codes. Anything else is a
/// pre-existing false negative these rules did not claim, or a new false
/// positive introduced by these rules.
#[test]
fn corpus_repro_emits_nothing_beyond_the_shipped_rules() {
    let src = repro_source();
    let unexpected: Vec<_> = code_lines(&src)
        .into_iter()
        .filter(|(c, _)| !matches!(*c, "RY102" | "RY103" | "RY105" | "RY107"))
        .collect();
    assert!(
        unexpected.is_empty(),
        "repro gained diagnostics beyond the shipped rules: {unexpected:?}"
    );
}
