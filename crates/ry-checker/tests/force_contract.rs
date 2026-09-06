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
            1,
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
