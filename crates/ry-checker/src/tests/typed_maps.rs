use super::*;

#[test]
fn regmatches_callbacks_do_not_assume_scalar_elements() {
    for source in [
        "x <- c('ab','z'); matches <- regmatches(x,regexec('(a)(b)',x)); Filter(function(z) length(z)>0L,matches)",
        "x <- c('aa','z'); matches <- regmatches(x,gregexpr('a',x)); lapply(matches,function(z) { if(length(z)==0L) return(z); toupper(z) })",
        "matches <- regmatches('ab',regexpr('a','ab'),invert=TRUE); lapply(matches,function(z) { if(length(z)==0L) return(z); z })",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY105"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn ordinary_character_callback_elements_still_have_scalar_length() {
    let diags = check("Filter(function(z) length(z)>0L,c('', 'ab'))");
    assert!(diags.iter().any(|d| d.code == "RY105"), "{diags:?}");
}

#[test]
fn indexed_sort_does_not_borrow_input_vector_shape() {
    for function in ["sort.int", "sort"] {
        let source = format!("x <- {function}(c(2,1),method='quick',index.return=TRUE); x$ix; x$x");
        let diags = check(&source);
        assert!(
            diags.iter().all(|d| d.code != "RY061"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn typed_callback_contract_is_independent_of_result_length() {
    for call in [
        "map_dbl(1, function(x) 'x')",
        "map2_dbl(1, 2, function(x, y) 'x')",
        "pmap_dbl(list(1, 2), function(x, y) 'x')",
        "map_int(1, function(x) c(1L, 2L))",
        "map_lgl(1, function(x) NULL)",
    ] {
        for source in [format!("purrr::{call}"), format!("library(purrr)\n{call}")] {
            let diags = check(&source);
            assert_eq!(diags.len(), 1, "{source}: {diags:?}");
            assert_eq!(
                (diags[0].code, diags[0].severity),
                ("RY080", Severity::Error)
            );
        }
    }
}

#[test]
fn empty_maps_and_compatible_callbacks_stay_silent() {
    for call in [
        "pmap_dbl(list(), function(...) 'x')",
        "map_dbl(numeric(0), function(x) 'x')",
        "map2_dbl(numeric(0), numeric(0), function(x, y) 'x')",
        "pmap_dbl(list(numeric(0), numeric(0)), function(x, y) 'x')",
        "map_dbl(1, function(x) TRUE)",
        "map_int(1, function(x) 1)",
        "map_lgl(1, function(x) TRUE)",
        "map_chr(1, function(x) 'x')",
        "map_dbl(1, function(x, extra) 'x', extra = NULL)",
    ] {
        let diags = check(&format!("purrr::{call}"));
        if call.contains("extra") {
            assert_eq!(
                diags.iter().filter(|d| d.code == "RY080").count(),
                1,
                "{diags:?}"
            );
        } else {
            assert!(diags.is_empty(), "{call}: {diags:?}");
        }
    }
    assert!(check("Position(function(x) TRUE, 1:3)").is_empty());
}

#[test]
fn pmap_binds_each_component_to_the_corresponding_callback_parameter() {
    let (_, scope) =
        check_with_scope("x <- purrr::pmap_dbl(list(1, 'x'), function(number, text) number)");
    assert_eq!(scope.get("x").map(|t| t.mode), Some(Mode::Double));
    let diags = check("purrr::pmap_dbl(list(1, 'x'), function(number, text) text)");
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY080").count(),
        1,
        "{diags:?}"
    );
}

#[test]
fn conditional_maps_and_accumulations_do_not_claim_one_callback_shape() {
    let source = "x <- purrr::map_if(list(1, 2), c(FALSE, TRUE), function(x) 'text')\nx[[1]] + 1\ny <- purrr::accumulate(1:3, function(x, y) x + y)";
    let (diags, scope) = check_with_scope(source);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(scope.get("y").unwrap().length, Length::Unknown);
}
