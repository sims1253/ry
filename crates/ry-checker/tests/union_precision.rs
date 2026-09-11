use ry_checker::Checker;
use ry_core::{Length, Mode, RParser};
use std::sync::Arc;

#[test]
fn concat_and_subset_keep_union_lengths_without_inventing_classes() {
    let stubs = tempfile::tempdir().unwrap();
    std::fs::write(
        stubs.path().join("example.json"),
        r#"{"version":"test","functions":{
        "mixed":{"params":[],"return":{"mode":"union","members":["opaque","integer"],"length":"1"}}
    }}"#,
    )
    .unwrap();
    let source = concat!(
        include_str!("../testdata/oracle/union_concat_subset.R"),
        "optional_value <- optional(TRUE)\n",
        "agreeing <- c(if (flag) 1L else 'a', 'b')\n",
        "plain <- c(NULL, 1L, 2L)\nlists <- c(list(1L), list(2L))\n",
        "mask <- function() { c <- function(...) TRUE; c(1L, 2L) }\nmasked <- mask()\n",
        "negative <- values[-1L]\nlogical_index <- values[c(TRUE, FALSE)]\n",
        "mixed <- example::mixed()\nrlang::check_data_frame(mixed)\noriginal <- example::mixed()\n",
    );
    let file = RParser::new().unwrap().parse("union.R", source).unwrap();
    let mut checker = Checker::new("union.R");
    checker.set_user_stubs(Arc::new(ry_typeshed::load_stub_dir(stubs.path()).unwrap()));
    let (diagnostics, scope) = checker.check_with_scope(&file);
    assert_eq!(
        diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        ["RY002"],
        "{diagnostics:?}"
    );
    for (name, length) in [
        ("optional_value", Length::Unknown),
        ("agreeing", Length::Known(2)),
        ("plain", Length::Known(2)),
        ("lists", Length::Known(2)),
        ("masked", Length::One),
        ("scalar", Length::One),
        ("negative", Length::One),
        ("logical_index", Length::Unknown),
    ] {
        assert_eq!(scope.get(name).unwrap().length, length, "{name}");
    }
    let scalar = scope.get("scalar").unwrap();
    assert_eq!(scalar.mode, Mode::Union);
    assert_eq!(
        scalar
            .members
            .as_ref()
            .unwrap()
            .iter()
            .map(|m| m.mode)
            .collect::<Vec<_>>(),
        [Mode::Integer, Mode::Character]
    );
    assert!(
        !scope
            .get("original")
            .unwrap()
            .members
            .as_ref()
            .unwrap()
            .iter()
            .find(|m| m.mode == Mode::Opaque)
            .unwrap()
            .class
            .known
    );
}
