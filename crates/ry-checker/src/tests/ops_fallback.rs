use super::*;

fn source(operator: &str, left: &str, right: &str) -> String {
    format!(
        "x <- structure(1L, class='{left}')\ny <- structure(2L, class='{right}')\n`{operator}.{left}` <- function(e1,e2) 1L\n`{operator}.{right}` <- function(e1,e2) 'right'\nchooseOpsMethod.{left} <- function(...) FALSE\nchooseOpsMethod.{right} <- function(...) FALSE\nout <- x {operator} y"
    )
}

#[test]
fn false_ops_choosers_use_scalar_primitive_modes_and_classes() {
    for (operator, mode, class) in [
        ("+", Mode::Integer, "left"),
        ("-", Mode::Integer, "left"),
        ("*", Mode::Integer, "left"),
        ("/", Mode::Double, "left"),
        ("^", Mode::Double, "left"),
        ("%%", Mode::Integer, "left"),
        ("%/%", Mode::Integer, "left"),
        ("==", Mode::Logical, ""),
        ("<", Mode::Logical, ""),
        ("&", Mode::Logical, ""),
        ("|", Mode::Logical, ""),
    ] {
        let source = source(operator, "left", "right");
        let (diagnostics, scope) = check_with_scope(&source);
        let result = scope.get("out").unwrap();
        assert_eq!(result.mode, mode, "{source}");
        assert_eq!(result.length, Length::One, "{source}");
        assert_eq!(
            result.class,
            if class.is_empty() {
                ClassVector::empty()
            } else {
                ClassVector::single(class)
            },
            "{source}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "RY051")
                .count(),
            1,
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn false_ops_choosers_bypass_factor_and_data_frame_methods() {
    for class in ["factor", "data.frame"] {
        let source = source("+", class, "right");
        let (diagnostics, scope) = check_with_scope(&source);
        let result = scope.get("out").unwrap();
        assert_eq!(result.mode, Mode::Integer, "{source}");
        assert!(result.class.contains(class), "{source}");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY051")
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY042")
        );
    }
}

#[test]
fn false_ops_fallback_needs_both_choices_and_different_method_modes() {
    let original = source("+", "left", "right");
    for source in [
        original.replace(
            "chooseOpsMethod.left <- function(...) FALSE",
            "chooseOpsMethod.left <- function(...) TRUE",
        ),
        original.replace(
            "chooseOpsMethod.right <- function(...) FALSE",
            "chooseOpsMethod.right <- function(...) TRUE",
        ),
        original.replace("chooseOpsMethod.right <- function(...) FALSE", ""),
        original.replace("chooseOpsMethod.left <- function(...) FALSE", ""),
        original.replace("function(e1,e2) 'right'", "function(e1,e2) 2L"),
        original.replace(
            "`+.right` <- function(e1,e2) 'right'",
            "`+.right` <- `+.left`",
        ),
        original.replace("structure(1L,", "structure(NULL,"),
        original.replace("out <-", "change_environment()\nout <-"),
    ] {
        let (diagnostics, _) = check_with_scope(&source);
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY051"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn false_ops_fallback_preserves_operand_order_and_primitive_errors() {
    let reversed = source("+", "left", "right").replace("out <- x + y", "out <- y + x");
    let (_, scope) = check_with_scope(&reversed);
    assert!(scope.get("out").unwrap().class.contains("right"));
    let invalid = source("+", "left", "right").replace("structure(1L,", "structure('text',");
    let (diagnostics, scope) = check_with_scope(&invalid);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY051")
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040")
    );
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn false_ops_fallback_does_not_extend_scalar_attribute_rules_to_other_lengths() {
    let prefix = source("+", "left", "right").replace("out <- x + y", "");
    let mut checker = Checker::new("test.R");
    let (_, scope) = checker.check_with_scope(&parse_file("test.R", &prefix));
    for length in [Length::Zero, Length::Known(2), Length::Unknown] {
        let mut lhs = scope.get("x").unwrap().clone();
        lhs.length = length;
        let result = crate::infer::ops_chooser::dispatch(
            &checker,
            "+",
            &lhs,
            scope.get("y").unwrap(),
            false,
            &scope,
        );
        assert!(
            matches!(result, Some(crate::infer::ops_chooser::Dispatch::Value(value)) if value.mode == Mode::Opaque),
            "{length:?}"
        );
    }
}

#[test]
fn plain_vector_fallback_rejects_both_empty_length_representations() {
    let prefix = source("+", "left", "right").replace("out <- x + y", "");
    let mut checker = Checker::new("test.R");
    let (_, scope) = checker.check_with_scope(&parse_file("test.R", &prefix));
    for length in [Length::Zero, Length::Known(0)] {
        for empty_on_left in [true, false] {
            let mut lhs = scope.get("x").unwrap().clone();
            let mut rhs = scope.get("y").unwrap().clone();
            if empty_on_left {
                lhs.length = length;
            } else {
                rhs.length = length;
            }
            let result =
                crate::infer::ops_chooser::dispatch(&checker, "+", &lhs, &rhs, true, &scope);
            assert!(
                matches!(result, Some(crate::infer::ops_chooser::Dispatch::Value(value)) if value.mode == Mode::Opaque),
                "{length:?}, empty_on_left={empty_on_left}"
            );
        }
    }
}
