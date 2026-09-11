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
    let source = "d <- 'TRUE'; if (flag) { d <- 'hello'; if (d) 1L } else { if (d) 1L }";
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
