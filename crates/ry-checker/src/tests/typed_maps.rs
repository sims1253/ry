use super::*;

#[test]
fn classed_callback_inputs_do_not_borrow_storage_elements() {
    for call in [
        "Filter(function(v) v + 1L > 0L, x)",
        "Position(function(v) v + 1L > 0L, x, right=TRUE)",
        "Find(function(v) v + 1L > 0L, x)",
        "lapply(x, function(v) v + 1L)",
    ] {
        let source = format!(
            "x <- structure(c('a','b'),class='ry_input')\nas.list.ry_input <- function(x,...) list(1L,2L)\n`[[.ry_input` <- function(x,i,...) 1L\n{call}"
        );
        let diagnostics = check(&source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY040"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn classed_empty_storage_does_not_prove_callbacks_are_skipped() {
    for call in [
        "Filter(function(v) 1L + 'bad', x)",
        "lapply(x, function(v) 1L + 'bad')",
    ] {
        let source = format!(
            "x <- structure(character(),class='ry_input')\nas.list.ry_input <- function(x,...) list(1L)\n{call}"
        );
        let diagnostics = check(&source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY040"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn classed_callback_results_do_not_borrow_input_length() {
    for call in [
        "lapply(x, function(v) 1L)",
        "vapply(x, function(v) 1L, integer(1), USE.NAMES=FALSE)",
        "sapply(x, function(v) 1L, USE.NAMES=FALSE)",
    ] {
        let source = format!(
            "as.list.ry_empty <- function(x,...) list()\nx <- structure('a',class='ry_empty')\nz <- {call}\nif (length(z) == 0L) 1L"
        );
        let (diagnostics, scope) = check_with_scope(&source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY105"),
            "{source}: {diagnostics:?}"
        );
        assert_eq!(scope.get("z").unwrap().length, Length::Unknown, "{call}");
        if call.starts_with("sapply") {
            assert_eq!(scope.get("z").unwrap().mode, Mode::Opaque);
        }
    }
}

#[test]
fn classed_dots_input_can_make_mapply_return_an_empty_list() {
    let source = "length.ry_zero <- function(x) 0L\nx <- structure('a',class='ry_zero')\nz <- mapply(function(v) 1L,x,USE.NAMES=FALSE)\nz$field";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(
        diagnostics.iter().all(|d| d.code != "RY061"),
        "{diagnostics:?}"
    );
    assert_eq!(scope.get("z").unwrap().mode, Mode::Opaque);
}

#[test]
fn callback_inputs_guard_classed_union_members_and_pmap_components() {
    for source in [
        "x <- if(flag) structure('a',class='ry_input') else 1L\nas.list.ry_input <- function(x,...) list(1L)\nlapply(x,function(v) v+1L)",
        "vec_proxy.ry_component <- function(x,...) list(1L)\nvec_restore.ry_component <- function(x,to,...) x\nx <- structure(list('a'),class='ry_component')\npurrr::pmap(list(x,1:2),function(v,other) v+other)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY040"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics = check("purrr::pmap(list(list('a'),1:2),function(v,other) v+other)");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY040"),
        "{diagnostics:?}"
    );
}

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

#[test]
fn recursive_apply_matches_how_and_keeps_unlisted_shapes_unknown() {
    for call in [
        "rapply(list(a = 1L), function(x) 1L, 'ANY', NULL, 'replace')",
        "rapply(list(a = 1L), function(x) 1L, ho = 'replace')",
        "rapply(list(a = 1L), function(x) 1L, how = 'rep')",
        "rapply(f = function(x) 1L, how = 'replace', object = list(a = 1L))",
    ] {
        let source = format!("result <- {call}; result$a");
        let diagnostics = check(&source);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    }
    for source in [
        "result <- rapply(list(1L), identity)",
        "result <- rapply(list(1L), identity, how = 'unlist')",
        "result <- rapply(list(1L), identity, how = 'u')",
        "result <- rapply(list(1L), function(x) NULL)",
        "how <- getOption('rapply_mode', 'unlist'); result <- rapply(list(1L), function(x) NULL, how = how)",
        "result <- rapply(list(1L), function(x) list())",
        "result <- rapply(list(list(1L, 2L)), function(x) c(x, x))",
        "result <- rapply(expression(1L), identity, how = 'replace')",
    ] {
        let (diagnostics, scope) = check_with_scope(source);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let result = scope.get("result").unwrap();
        assert_eq!(
            (result.mode, result.length),
            (Mode::Opaque, Length::Unknown),
            "{source}: {result:?}"
        );
    }
}

#[test]
fn recursive_apply_literal_list_modes_preserve_top_level_length() {
    for how in ["list", "replace", "l", "r"] {
        let source =
            format!("result <- rapply(list(1L, 2L), function(x) c(1L, 2L), how = '{how}')");
        let (diagnostics, scope) = check_with_scope(&source);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let result = scope.get("result").unwrap();
        assert_eq!(
            (result.mode, result.length),
            (Mode::List, Length::Known(2)),
            "{source}: {result:?}"
        );
    }
}

#[test]
fn recursive_apply_replace_preserves_outer_class_without_element_claims() {
    let (diagnostics, scope) = check_with_scope(
        "`+.widget` <- function(e1, e2) 7L; input <- structure(list(a = 1L), class = 'widget'); result <- rapply(input, identity, how = 'replace'); result + 1L",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let result = scope.get("result").unwrap();
    assert!(result.class.contains("widget"));
    assert!(result.columns.is_none());
}
