use super::*;

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
