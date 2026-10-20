//! Message wording of the recall rules RY105 and RY107. The firing and
//! silent cases for RY102, RY103, RY105, and RY107 live in
//! `testdata/cases/recall_rules.R`.

use ry_checker::Checker;
use ry_core::RParser;

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
fn ry107_negative_literal_dead_guard_keeps_the_element_rewrite() {
    // The dead always-TRUE guard still suggests moving the comparison to
    // the elements, with the folded literal spelled as written. The
    // constant claim is conditioned on the base result not being NA
    // (R: `any(c(FALSE, NA)) > -1` is NA), never asserted unconditionally.
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse("recall.R", "f <- function(x) if (any(x) > -1) 1\n")
        .expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == "RY107"
            && d.message
                .contains("always TRUE when the base result is not `NA`")
            && d.message.contains("any(x > -1)")),
        "expected the conditioned constant wording with the element-level rewrite: {diags:?}"
    );
}

#[test]
fn ry107_constant_claims_are_conditioned_on_the_na_result() {
    // R (4.6.1, fresh process): `any(c(FALSE, NA))` is NA, and `NA > -1`
    // and `NA == 2` are NA -- the constant outcome holds only for the
    // base domain's non-NA results, so both truth values are worded as
    // "when the base result is not `NA`" with the NA case named.
    for (src, fragment) in [
        (
            "f <- function(x) if (any(x) > -1) 1\n",
            "always TRUE when the base result is not `NA` (an `NA` result compares as `NA`)",
        ),
        (
            "f <- function(x) if (any(x) == 2) 1\n",
            "always FALSE when the base result is not `NA` (an `NA` result compares as `NA`)",
        ),
    ] {
        let mut parser = RParser::new().expect("parser init");
        let file = parser.parse("recall.R", src).expect("parse");
        let mut checker = Checker::new("recall.R");
        checker.check(&file);
        let diags = checker.take_diagnostics();
        assert!(
            diags
                .iter()
                .any(|d| d.code == "RY107" && d.message.contains(fragment)),
            "expected conditioned constant wording in {src:?}: {diags:?}"
        );
    }
}

#[test]
fn ry107_negating_wording_names_its_na_case() {
    // `any(x) == 0` is NA -- not TRUE -- when the call returns NA, so the
    // negating wording states its own NA case instead of implying a total
    // TRUE/FALSE function.
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse("recall.R", "f <- function(x) if (any(x) == 0) 1\n")
        .expect("parse");
    let mut checker = Checker::new("recall.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == "RY107"
            && d.message
                .contains("TRUE exactly when the call is FALSE (`NA` when it is `NA`)")),
        "expected the negating wording to name the NA case: {diags:?}"
    );
}

#[test]
fn ry107_open_world_arguments_keep_the_dispatch_qualified_premise() {
    // any()/all() are S4 generics (R 4.6.1, fresh process): a direct
    // `setMethod("any", ...)` returns 42, a `Summary`-group method returns
    // c(5, 7), and `base::any()` still selects the generic rather than a
    // default method -- so the length-1 premise is qualified in the
    // message ("unless an `any`/`all` or `Summary`-group method
    // dispatches") instead of being stated as unconditional. These
    // arguments are open-world, so the rule keeps firing: the founding
    // glue shape passes a parameter, and silencing every argument a
    // caller could have classed would erase the rule's purpose. The
    // qualifier names exactly that residual dispatch risk.
    for src in [
        "f <- function(x) if (any(x) == 0) 1\n",
        "f <- function(x) if (base::any(x) > -1) 1\n",
        // A defaulted parameter stays open-world: the default's shape says
        // nothing about the caller's values.
        "f <- function(x = c(1, 2)) if (any(x) == 0) 1\n",
    ] {
        let mut parser = RParser::new().expect("parser init");
        let file = parser.parse("recall.R", src).expect("parse");
        let mut checker = Checker::new("recall.R");
        checker.check(&file);
        let diags = checker.take_diagnostics();
        assert!(
            diags.iter().any(|d| d.code == "RY107"
                && d.message
                    .contains("unless an `any`/`all` or `Summary`-group method dispatches")),
            "expected the dispatch-qualified premise in {src:?}: {diags:?}"
        );
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
