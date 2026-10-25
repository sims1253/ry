use super::*;

#[test]
fn assigned_character_literals_are_checked_in_conditions() {
    for source in [
        "d <- 'hello'; if (d) 1L",
        "d <- 'NA'; alias <- d; if (alias) 1L",
        "a <- d <- 'hello'; if (d) 1L",
        "`flag value` <- 'hello'; if (`flag value`) 1L",
        "d <- 'hello'; while (d) break",
        "f <- function() { d <- 'hello'; if (d) 1L }; f()",
        "d <- 'TRUE'; d <- 'hello'; if (d) 1L",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn accepted_literals_and_invalidated_values_stay_silent() {
    for source in [
        "d <- 'TRUE'; if (d) 1L",
        "d <- 'hello'; d <- 'False'; if (d) 1L",
        "d <- 'hello'; d[1L] <- 'TRUE'; if (d) 1L",
        "d <- 'hello'; if (flag) d <- 'TRUE'; if (d) 1L",
        "d <- 'TRUE'; if (flag) d <- 'hello'; if (d) 1L",
        "d <- 'hello'; f <- function() { if (d) 1L }; d <- 'TRUE'; f()",
        "f <- function(d = 'hello') { if (d) 1L }; f('TRUE')",
        "d <- 'hello'; mutate(); if (d) 1L",
        "FALSE || { mutate(); TRUE }; d <- 'hello'; if (d) 1L",
        "if (flag) mutate(); d <- 'hello'; if (d) 1L",
        "result <- if (flag) mutate() else 1L; d <- 'hello'; if (d) 1L",
        "d <- 'hello'; for (i in 1:2) d <- 'TRUE'; if (d) 1L",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn branch_literal_values_do_not_leak_into_sibling_branches() {
    let source =
        "flag <- TRUE; d <- 'TRUE'; if (flag) { d <- 'hello'; if (d) 1L } else { if (d) 1L }";
    let diagnostics = check(source);
    let invalid: Vec<_> = diagnostics.iter().filter(|d| d.code == "RY001").collect();
    assert_eq!(invalid.len(), 1, "{diagnostics:?}");
    assert_eq!(invalid[0].span.start, source.find("if (d)").unwrap() + 4);
}

#[test]
fn active_bindings_and_custom_assignment_do_not_supply_literal_values() {
    for source in [
        "makeActiveBinding('d', function(value) 'TRUE', globalenv()); d <- 'hello'; if (d) 1L",
        "`<-` <- function(x, value) NULL; d <- 'hello'; if (d) 1L",
        "d <- 'TRUE'; f <- function() { d <- 'TRUE'; d <<- 'hello'; if (d) 1L }; f()",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn assigned_character_condition_oracle_retains_the_warning() {
    let diagnostics = check(include_str!(
        "../../testdata/oracle/assigned_character_conditions.R"
    ));
    assert!(
        diagnostics.iter().any(|d| d.code == "RY001"),
        "{diagnostics:?}"
    );
}

#[test]
fn method_and_replacement_effects_block_later_literal_claims() {
    for source in [
        "`+.flag` <- function(a, b) { makeActiveBinding('e', function(value) 'TRUE', globalenv()); a }; d <- 1L; class(d) <- 'flag'; x <- d + 1L; e <- 'hello'; if (e) 1L",
        "`activate<-` <- function(x, value) { makeActiveBinding('e', function(value) 'TRUE', globalenv()); x }; d <- 1L; activate(d) <- 1L; e <- 'hello'; if (e) 1L",
        "`+.flag` <- function(a, b) { makeActiveBinding('e', function(value) 'TRUE', globalenv()); a }; d <- base::structure(1L, class = 'flag'); x <- d + 1L; e <- 'hello'; if (e) 1L",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics = check("d <- 1L; x <- d + 1L; e <- 'hello'; if (e) 1L");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY001"),
        "{diagnostics:?}"
    );
}

#[test]
fn forced_promises_block_later_literal_claims() {
    for source in [
        "f <- function(x) { x; e <- 'hello'; if (e) 1L }",
        "unknown_value; e <- 'hello'; if (e) 1L",
        include_str!("../../testdata/oracle/literal_conditions_after_promises.R"),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
}

// Issue #362: the NULL component of a union return type must reach the
// condition analysis at the call site. The four probes below mirror the
// audit artifacts that isolated the boundary: local NULL flow and
// single-return helpers already fired; a find-or-NULL helper's
// `character | NULL` union lost the NULL member in both `if` (through a
// comparison, which joins to `logical<0> | logical<1>`) and `switch`
// (the EXPR selector) conditions.
#[test]
fn null_union_return_components_reach_condition_analysis() {
    let locate = "locate_input <- function(input) {\n  if (is.null(input)) {\n    return(NULL)\n  }\n  \"path\"\n}\n";
    for (note, tail) in [
        (
            "switch on the union",
            "where <- locate_input(NULL)\nswitch(where, path = 1L, 2L)\n",
        ),
        (
            "if on a comparison over the union",
            "where <- locate_input(NULL)\nif (where == \"path\") 1L else 2L\n",
        ),
        (
            "while on a comparison over the union",
            "where <- locate_input(NULL)\nwhile (where == \"path\") {\n  break\n}\n",
        ),
        (
            "direct condition on the union",
            "where <- locate_input(NULL)\nif (where) 1L else 2L\n",
        ),
        (
            "local NULL flow through a comparison",
            "where <- NULL\nif (where == \"path\") 1L else 2L\n",
        ),
        (
            "single-return NULL helper through a comparison",
            "get_null <- function() NULL\nwhere <- get_null()\nif (where == \"path\") 1L else 2L\n",
        ),
        (
            "local NULL flow into a switch selector",
            "where <- NULL\nswitch(where, path = 1L, 2L)\n",
        ),
        (
            "literal NULL switch selector",
            "switch(NULL, path = 1L, 2L)\n",
        ),
    ] {
        let source = format!("{locate}{tail}");
        let diagnostics = check(&source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY001"),
            "{note}: the NULL component must flag the condition: {diagnostics:?}"
        );
    }
}

// Issue #354: a condition that is syntactically a proven scalar `NA`
// always errors in R ("missing value where TRUE/FALSE needed" for a
// logical NA, "argument is not interpretable as logical" for the typed
// NAs). Only closed literal shapes count as proof here; bindings and
// possibly-NA values stay with the follow-up work recorded on #354.
#[test]
fn proven_na_literal_conditions_are_checked() {
    for source in [
        "if (NA) 1L",
        "while (NA) break",
        "if (NA_integer_) 1L",
        "if (NA_real_) 1L",
        "if (NA_character_) 1L",
        "if (NA_complex_) 1L",
        "if (!NA) 1L",
        "if (-NA_real_) 1L",
        "x <- if (NA) 1L else 2L",
        // Mixed-type comparisons coerce in R and still yield NA.
        "if (NA_integer_ < \"x\") 1L",
        "if (NA_character_ < TRUE) 1L",
        "if (NA_complex_ == 1) 1L",
        "if (NA_integer_ > 1L) 1L",
        "if (1L < NA_integer_) 1L",
        "if (NA + 1L) 1L",
        "if (any(NA)) 1L",
        "if (all(NA)) 1L",
        "if (sum(NA_integer_)) 1L",
        "if (c(NA)) 1L",
        "if (as.logical(NA_integer_)) 1L",
        "if (as.integer(1e10)) 1L",
        "f <- function() { if (NA) 1L }; f()",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics = check("if (NA) 1L");
    let invalid: Vec<_> = diagnostics.iter().filter(|d| d.code == "RY001").collect();
    assert_eq!(invalid.len(), 1, "{diagnostics:?}");
    assert!(
        invalid[0].message.contains("always `NA`"),
        "the message must identify the proven-NA condition: {diagnostics:?}"
    );
    // A longer out-of-range cast is a length error, not a scalar NA:
    // RY119 fires, but the message must not claim an always-NA scalar.
    let diagnostics = check("if (as.integer(c(1, 1e10))) 1L");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY119"),
        "the cast itself must still be diagnosed: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code != "RY001" || !d.message.contains("always `NA`")),
        "{diagnostics:?}"
    );
}

// The counterexamples that dropped the earlier value-fact slice on
// #354, plus the guarded and possibly-NA idioms, must stay silent: a
// binding's recorded NA fact can be invalidated by replacement,
// subsetting, `Ops` dispatch, character coercion of `NaN`, or an
// argument supplied over a default.
#[test]
fn guarded_and_unproven_na_conditions_stay_silent() {
    for source in [
        "if (!is.na(x)) 1L",
        "if (isTRUE(x)) 1L",
        "if (x) 1L",
        "if (sum(x)) 1L",
        "if (any(x)) 1L",
        "x <- NA; if (x) 1L",
        "x <- NA; x[1L] <- TRUE; if (x) 1L",
        "x <- c(NA_real_, 1); if (x[2L]) 1L",
        "if (NaN == \"NaN\") 1L",
        "Ops.foo <- function(e1, e2) TRUE; x <- if (flag) base::structure(1, class = \"foo\") else 1; if (x > NA_real_) 1L",
        "f <- function(x = NA) x; if (f(TRUE)) 1L",
        "if (any(NA, na.rm = TRUE)) 1L",
        "if (NA & FALSE) 1L",
        "if (NA^0) 1L",
        "any <- function(x, ...) TRUE; if (any(NA)) 1L",
        "if (NA == x) 1L",
        // Ordering a complex NA errors in the comparison, not as an NA.
        "if (NA_complex_ > 1) 1L",
        "if (1 < NA_complex_) 1L",
        // A rebound unary `-` can make `-1` a classed value whose Ops
        // method decides the comparison (the branch runs fine in R).
        "`-` <- function(e1) base::structure(1, class = \"foo\"); Ops.foo <- function(e1, e2) TRUE; if (NA == -1) 1L",
        "`-` <- function(e1) base::structure(1, class = \"foo\"); Ops.foo <- function(e1, e2) TRUE; if (NA + -1.5) 1L",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
}

// The same unions without a NULL member, and non-condition uses of a
// NULL-carrying union, stay silent: the rule is about condition
// positions on the possibly-NULL value, nothing else.
#[test]
fn null_free_unions_and_non_condition_uses_stay_silent() {
    let pick = "pick <- function(x) {\n  if (x > 0) {\n    \"path\"\n  } else {\n    1L\n  }\n}\n";
    for (note, tail) in [
        (
            "union without NULL in a switch",
            "w <- pick(1)\nswitch(w, path = 1L, 2L)\n",
        ),
        (
            "union without NULL in a comparison condition",
            "w <- pick(1)\nif (w == \"path\") 1L else 2L\n",
        ),
    ] {
        let source = format!("{pick}{tail}");
        let diagnostics = check(&source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{note}: {diagnostics:?}"
        );
    }
    let locate = "locate_input <- function(input) {\n  if (is.null(input)) {\n    return(NULL)\n  }\n  \"path\"\n}\n";
    let diagnostics = check(&format!(
        "{locate}where <- locate_input(NULL)\nprint(where)\ny <- paste0(where, \"!\")\nz <- where[[1L]]\n"
    ));
    assert!(
        diagnostics.iter().all(|d| d.code != "RY001"),
        "non-condition uses must stay silent: {diagnostics:?}"
    );
}
