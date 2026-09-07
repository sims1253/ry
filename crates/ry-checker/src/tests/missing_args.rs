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
