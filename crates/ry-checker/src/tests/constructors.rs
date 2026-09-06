use super::*;

#[test]
fn vector_sizes_use_values_and_r_argument_matching() {
    for (name, mode) in [
        ("integer", Mode::Integer),
        ("numeric", Mode::Double),
        ("double", Mode::Double),
        ("logical", Mode::Logical),
        ("character", Mode::Character),
        ("raw", Mode::Raw),
    ] {
        for (argument, length) in [
            ("", Length::Zero),
            ("0", Length::Zero),
            ("1", Length::One),
            ("10", Length::Known(10)),
            ("len = 3", Length::Known(3)),
            ("length = 2.9", Length::Known(2)),
            ("-0.2", Length::Zero),
            ("-2", Length::Unknown),
            ("NA_real_", Length::Unknown),
            ("Inf", Length::Unknown),
            ("size", Length::Unknown),
        ] {
            for callee in [name.to_string(), format!("base::{name}")] {
                let (_, scope) =
                    check_with_scope(&format!("size <- readLines()\nx <- {callee}({argument})"));
                assert_eq!(
                    scope.get("x"),
                    Some(&RType::new(mode, length)),
                    "{callee}({argument})"
                );
            }
        }
    }
    for (arguments, mode, length) in [
        ("", Mode::Logical, Length::Zero),
        ("length = 3", Mode::Logical, Length::Known(3)),
        ("length = 2, 'integer'", Mode::Integer, Length::Known(2)),
        ("len = 2, mo = 'double'", Mode::Double, Length::Known(2)),
    ] {
        let (diags, scope) = check_with_scope(&format!("x <- vector({arguments})"));
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(scope.get("x"), Some(&RType::new(mode, length)));
    }
}

#[test]
fn local_constructors_override_base_metadata() {
    let (diags, scope) = check_with_scope("numeric <- function(length) 'local'\nx <- numeric(10)");
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(scope.get("x"), Some(&RType::scalar(Mode::Character)));
}
