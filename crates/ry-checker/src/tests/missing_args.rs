use super::*;

#[test]
fn missing_actuals_still_occupy_their_formals() {
    let file = parse_file("test.R", "f(, second=2L, 3L)");
    let [Stmt::Expr(Expr::Call { args, .. })] = file.stmts.as_slice() else {
        panic!("{:?}", file.stmts)
    };
    let matched = crate::infer::match_arguments(&["first", "second", "third"], args);
    assert_eq!(matched.param_for_arg, vec![Some(0), Some(1), Some(2)]);
    assert!(matches!(args[0].value, Expr::Missing(_)));
}

#[test]
fn missing_values_cannot_prove_constructor_payloads_or_closed_records() {
    let (_, scope) =
        check_with_scope("x <- structure(.Data=, class='widget'); y <- list(a=, b=1L)");
    assert_eq!(scope.get("x").unwrap().mode, Mode::Opaque);
    assert!(!scope.get("x").unwrap().class.contains("widget"));
    assert!(!scope.get("y").unwrap().columns.as_ref().unwrap().complete);
}

#[test]
fn preserved_empty_row_index_still_selects_the_column() {
    let (_, scope) = check_with_scope(
        "d <- data.frame(value=1L, other='x'); x <- d[, 'value']; y <- d[, 'other']",
    );
    assert_eq!(scope.get("x").unwrap().mode, Mode::Integer);
    assert_eq!(scope.get("y").unwrap().mode, Mode::Character);
}

#[test]
fn omitted_actual_does_not_turn_invalid_arity_into_a_forcing_contract() {
    // R rejects the extra actual before identity can force x. Dropping the
    // omitted first slot incorrectly made this look like identity(x).
    let diagnostics = check("f <- function(x=x) base::identity(, x)");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY098"),
        "{diagnostics:?}"
    );
}

#[test]
fn omitted_payload_cannot_borrow_the_next_actuals_type() {
    let (_, scope) = check_with_scope("out <- base::identity(, 1L)");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
    let diagnostics = check("Filter()");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY091"),
        "{diagnostics:?}"
    );
}
