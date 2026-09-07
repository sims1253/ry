use super::*;

#[test]
fn simplify_false_keeps_sapply_and_mapply_results_as_lists() {
    for source in [
        "a <- sapply(1L, function(v) 1L, simplify = FALSE); a$field",
        "a <- sapply(simplify = FALSE, FUN = function(v) 1L, X = 1L); a$field",
        "b <- mapply(SIMPLIFY = FALSE, FUN = function(x) 1L, x = 1L); b$field",
        "b <- mapply(FUN = function(x) 1L, x = 1L, SIMPLIFY = FALSE); b$field",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY061"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn unknown_simplify_controls_do_not_prove_atomic_results() {
    for source in [
        "control <- identity(FALSE); a <- sapply(1L, function(v) 1L, simplify = control); a$field",
        "control <- identity(FALSE); b <- mapply(FUN = function(x) 1L, x = 1L, SIMPLIFY = control); b$field",
        "library(dplyr); c <- sapply(1L, function(v) 1L, simplify = FALSE); c$field",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY061"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn non_base_simplify_signatures_keep_their_existing_contract() {
    let stub = r#"{
        "schema_version": "2",
        "package": "custom",
        "version": "test",
        "functions": {
            "sapply": {
                "params": [
                    {"name": "X", "required": true},
                    {"name": "FUN", "required": true},
                    {"name": "simplify", "default": true}
                ],
                "higher_order": {
                    "callback_param": "FUN",
                    "callback_position": 1,
                    "callback_args": ["element_of_arg0"],
                    "result": {"kind": "simplify", "length_arg": 0}
                },
                "return": {"mode": "opaque", "length": "unknown"}
            }
        }
    }"#;
    let base_stub = stub.replace("\"package\": \"custom\"", "\"package\": \"base\"");
    for (source, file, contents) in [
        (
            "a <- custom::sapply(1L, function(v) 1L, simplify = FALSE); a$field",
            "custom.json",
            stub,
        ),
        (
            "a <- sapply(1L, function(v) 1L, simplify = FALSE); a$field",
            "base.json",
            base_stub.as_str(),
        ),
    ] {
        let diagnostics = check_with_stubs(source, &[(file, contents)]).0;
        assert!(
            diagnostics.iter().any(|d| d.code == "RY061"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn unknown_higher_order_input_length_does_not_prove_simplification() {
    for source in [
        "x <- integer(1L - 1L); a <- sapply(x, function(v) 1L, USE.NAMES = FALSE); a$field",
        "x <- integer(1L - 1L); b <- mapply(function(v) 1L, x, USE.NAMES = FALSE); b$field",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY061"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn known_nonempty_scalar_simplification_still_reports_atomic_dollar() {
    for source in [
        "a <- sapply(1L, function(v) 1L, simplify = TRUE); a$field",
        "b <- mapply(FUN = function(x) 1L, x = 1L, SIMPLIFY = TRUE); b$field",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY061"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn simplify_false_does_not_skip_callback_diagnostics() {
    for source in [
        "sapply(1L, function(v) v + 'bad', simplify = FALSE)",
        "mapply(function(x) x + 'bad', x = 1L, SIMPLIFY = FALSE)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY040"),
            "{source}: {diagnostics:?}"
        );
    }
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
