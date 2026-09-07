use std::collections::BTreeMap;
use std::sync::Arc;

use ry_checker::Checker;
use ry_core::RParser;
use ry_typeshed::Typeshed;

fn recursive_warnings(source: &str, stubs: BTreeMap<String, Typeshed>) -> usize {
    let file = RParser::new().unwrap().parse("force.R", source).unwrap();
    let mut checker = Checker::new("force.R");
    checker.set_user_stubs(Arc::new(stubs));
    checker
        .check(&file)
        .iter()
        .filter(|diag| diag.code == "RY098")
        .count()
}

fn fixture() -> Typeshed {
    serde_json::from_str(r#"{"package":"fixture","version":"test","functions":{
        "strict":{"params":["value"],"return":"arg0","force":{"kind":"sole_argument","param":"value","allow_named":true}},
        "unnamed":{"params":["..."],"return":"arg0","force":{"kind":"sole_argument","param":"...","allow_named":false}},
        "halt":{"params":["..."],"return":"arg0","no_return":true},
        "ordinary":{"params":["value"],"return":"arg0","eval":{"value":"normal"}},
        "capture":{"params":["value"],"return":"arg0","eval":{"value":"quoted_expression"}}
    }}"#).unwrap()
}

#[test]
fn qualified_force_contract_uses_metadata_and_requires_reviewed_call_shape() {
    for (call, expected) in [
        ("fixture::strict(x)", 1),
        ("fixture:::strict(value = x)", 1),
        ("fixture::unnamed(x)", 1),
        ("fixture::strict(other = x)", 0),
        ("fixture::strict(val = x)", 0),
        ("fixture::strict(x, x)", 0),
        ("fixture::strict(value = )", 0),
        ("fixture::strict()", 0),
        ("fixture::strict(...)", 0),
        ("fixture::unnamed(value = x)", 0),
        ("fixture::ordinary(x)", 0),
        ("fixture::capture(x)", 0),
        ("fixture::strict(if (FALSE) x else 1L)", 0),
        ("fixture::strict(FALSE && x)", 0),
        ("fixture::strict(fixture::halt() + x)", 0),
        ("fixture::strict(fixture::halt()[x])", 0),
        ("object[fixture::strict(x)]", 0),
        ("object[[fixture::strict(x)]]", 0),
        ("fixture::strict(x)[1L]", 1),
        ("fixture::strict(if (flag) x else 1L)", 0),
        ("fixture::strict(if (TRUE) x else 1L)", 1),
        ("fixture::strict(function() x)", 0),
        ("fixture::strict({ fixture::halt(); x })", 0),
        ("fixture::strict({ y <- fixture::halt(); x })", 0),
        (
            "fixture::strict({ fixture::strict(fixture::halt()); x })",
            0,
        ),
        (
            "fixture::strict({ fixture::strict({ fixture::halt() }); x })",
            0,
        ),
        (
            "fixture::strict({ fixture::ordinary(fixture::halt()); x })",
            0,
        ),
        ("fixture::strict({ return(1L); x })", 0),
        (
            "fixture::strict({ fixture::strict(x); fixture::halt() })",
            1,
        ),
        ("other::strict(x)", 0),
        ("strict(x)", 0),
    ] {
        let source = format!("f <- function(x = x, flag = FALSE, ...) {call}");
        assert_eq!(
            recursive_warnings(&source, BTreeMap::from([("fixture".into(), fixture())])),
            expected,
            "{call}"
        );
    }
}

#[test]
fn replacement_base_stub_controls_forcing_without_builtin_fallback() {
    let mut base = ry_typeshed::load_base().unwrap();
    base.functions.get_mut("identity").unwrap().force = None;
    assert_eq!(
        recursive_warnings(
            "f <- function(x = x) base::identity(x)",
            BTreeMap::from([("base".into(), base)])
        ),
        0
    );
    assert_eq!(
        recursive_warnings("f <- function(x = x) stats::identity(x)", BTreeMap::new()),
        0
    );
}

#[test]
fn default_blocks_stop_before_unreachable_or_replaced_reads() {
    for (default, expected) in [
        ("{ fixture::halt(); fixture::strict(x) }", 0),
        ("fixture::halt() + fixture::strict(x)", 0),
        ("if (fixture::halt()) fixture::strict(x) else 1L", 0),
        ("{ if (TRUE) fixture::halt(); fixture::strict(x) }", 0),
        (
            "{ if (flag) fixture::halt() else fixture::halt(); fixture::strict(x) }",
            0,
        ),
        ("{ while (fixture::halt()) fixture::strict(x) }", 0),
        ("{ for (i in fixture::halt()) fixture::strict(x) }", 0),
        ("fixture::strict(x) + fixture::halt()", 1),
        ("fixture::halt()[fixture::strict(x)]", 0),
        ("matrix[fixture::halt(), fixture::strict(x)]", 0),
        ("{ return(1L); fixture::strict(x) }", 0),
        ("{ x <- 1L; fixture::strict(x) }", 0),
        ("{ x <- fixture::strict(x); 1L }", 1),
        ("{ if (TRUE) x <- 1L else x <- 2L; fixture::strict(x) }", 0),
        ("{ if (flag) x <- 1L else x <- 2L; fixture::strict(x) }", 0),
        ("{ if (flag) x <- 1L; fixture::strict(x) }", 0),
        ("{ inner <- function() { x <- 1L }; fixture::strict(x) }", 1),
        ("{ fixture::strict(x); fixture::halt() }", 1),
        (
            "{ if (flag) { fixture::halt(); fixture::strict(x) } else 1L }",
            0,
        ),
        ("{ if (flag) 1L else { x <- 1L; fixture::strict(x) } }", 0),
    ] {
        let source = format!("f <- function(flag = FALSE, x = {default}) x");
        assert_eq!(
            recursive_warnings(&source, BTreeMap::from([("fixture".into(), fixture())])),
            expected,
            "{default}"
        );
    }
}

#[test]
fn opaque_prefixes_cannot_preserve_the_original_promise() {
    for (source, expected) in [
        (
            "f <- function(x = x) { assign('x', 1L); fixture::strict(x) }",
            0,
        ),
        (
            "f <- function(x = { assign('x', 1L); fixture::strict(x) }) x",
            0,
        ),
        (
            "f <- function(x = x) { y <- (x <- 1L); fixture::strict(x) }",
            0,
        ),
        ("f <- function(x = (x <- 1L) + fixture::strict(x)) x", 0),
        (
            "f <- function(x = { if ((x <- TRUE)) 1L; fixture::strict(x) }) x",
            0,
        ),
        ("f <- function(x = x) { helper(); fixture::strict(x) }", 0),
        (
            "f <- function(x = x, y = NULL) { y; fixture::strict(x) }",
            0,
        ),
        (
            "f <- function(x = x) { fixture::strict(1L); fixture::strict(x) }",
            0,
        ),
        (
            "f <- function(flag, x = if (flag) fixture::strict(x) else 1L) x",
            0,
        ),
        (
            "f <- function(x = x) { 1L; NULL; y <- 2L; fixture::strict(x) }",
            1,
        ),
        (
            "f <- function(x = x) { y <- fixture::strict(x); helper() }",
            1,
        ),
    ] {
        assert_eq!(
            recursive_warnings(source, BTreeMap::from([("fixture".into(), fixture())])),
            expected,
            "{source}"
        );
    }
}

#[test]
fn quoted_binding_spellings_are_not_treated_as_independent() {
    for (source, expected) in [
        ("f <- function(x = x) { `x` <- 1L; fixture::strict(x) }", 0),
        ("f <- function(x = (`x` <- 1L) + fixture::strict(x)) x", 0),
        ("f <- function(`x` = `x`) fixture::strict(`x`)", 1),
    ] {
        assert_eq!(
            recursive_warnings(source, BTreeMap::from([("fixture".into(), fixture())])),
            expected,
            "{source}"
        );
    }
}

#[test]
fn pipes_and_visible_syntax_rebindings_do_not_prove_forcing() {
    for source in [
        "f <- function(x = x) base::typeof(x) |> (function(z) 1L)()",
        "f <- function(x = base::typeof(x) |> (function(z) 1L)()) x",
        "f <- function(x = x) base::typeof(x) %>% (function(z) 1L)()",
        "f <- function(x = x) base::typeof(x) %T>% (function(z) 1L)()",
        "f <- function(x = x) x %<>% (function(z) 1L)()",
        "f <- function(x = x) base::typeof(x) %in% 1L",
        "`+` <- function(a, b) 1L; f <- function(x = x) base::typeof(x) + 1L",
        "f <- function(`+`, x = x) base::typeof(x) + 1L",
        "f <- function(x = x) { `+` <- function(a, b) 1L; base::typeof(x) + 1L }",
        "`!` <- function(a) TRUE; f <- function(x = x) !base::typeof(x)",
        "f <- function(`+`, x = base::typeof(x) + 1L) x",
    ] {
        assert_eq!(recursive_warnings(source, BTreeMap::new()), 0, "{source}");
    }
    assert_eq!(
        recursive_warnings("f <- function(x = x) base::typeof(x) + 1L", BTreeMap::new()),
        1
    );
}

#[test]
fn unknown_column_selection_does_not_guarantee_a_loop_body_force() {
    let source = r#"
        columns <- function(data = data, classes = classes) {
            selected <- names(classes[classes == "character"])
            for (column in selected) {
                value <- data[[column]]
            }
            classes
        }
    "#;
    assert_eq!(recursive_warnings(source, BTreeMap::new()), 0);
}

#[test]
fn opaque_conditions_remain_barriers_even_when_all_branches_read_default() {
    for source in [
        "f <- function(flag, x = if (flag) x else x) base::force(x)",
        "f <- function(flag, x = flag && x || x) base::force(x)",
    ] {
        assert_eq!(recursive_warnings(source, BTreeMap::new()), 0, "{source}");
    }
}

#[test]
fn visible_namespace_and_function_syntax_bindings_disable_force_assumptions() {
    for source in [
        r"`\x3a\x3a` <- function(pkg, name) function(...) 1L; f <- function(x = x) base::typeof(x)",
        r"f <- function(`\x3a\x3a`, x = x) base::typeof(x)",
        "`::` <- function(pkg, name) function(...) 1L; f <- function(x = x) base::typeof(x)",
        "`:::` <- function(pkg, name) function(...) 1L; f <- function(x = x) base:::typeof(x)",
        "f <- function(`::`, x = x) base::typeof(x)",
        "f <- function(`:::`, x = x) base:::typeof(x)",
        "`function` <- function(...) 1L; f <- function(x = x) x",
        "outer <- function(`function`) { f <- function(x = x) x }",
    ] {
        assert_eq!(recursive_warnings(source, BTreeMap::new()), 0, "{source}");
    }
}

#[test]
fn project_syntax_bindings_survive_table_merging_without_literal_functions() {
    for binding in [r"`\x3a\x3a`", "`::`"] {
        for rhs in [
            "function(pkg, name) function(...) 1L",
            "lazy",
            "make_namespace()",
        ] {
            let mut parser = RParser::new().unwrap();
            let mut project = ry_checker::Project::new();
            let operator = format!("{binding} <- {rhs}");
            for (path, source) in [
                ("operator.R", operator.as_str()),
                ("caller.R", "f <- function(x = x) base::typeof(x)"),
            ] {
                project.add_file(path.into(), parser.parse(path, source).unwrap());
            }
            assert!(
                project.check().iter().all(|(_, diagnostics)| {
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.code != "RY098")
                }),
                "{operator}"
            );
        }
    }
}
