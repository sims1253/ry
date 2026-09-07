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

#[test]
fn required_missing_actuals_are_unfilled_but_optional_defaults_remain_valid() {
    for source in ["Filter()", "Filter(, x=1L)", "Filter(f=, x=1L)"] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY091"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics = check("round(1.25, digits=)");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY091"),
        "{diagnostics:?}"
    );
}

#[test]
fn duplicate_payload_names_cannot_lend_constructor_facts() {
    let (_, scope) = check_with_scope("out <- structure(.Data=, .Data=1L, class='widget')");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
    assert!(!scope.get("out").unwrap().class.contains("widget"));
}

#[test]
fn omitted_initializer_actual_cannot_prove_an_s4_instance() {
    let (_, scope) = check_with_scope(
        "methods::setClass('Widget', slots=c(value='integer')); out <- methods::new('Widget', value=)",
    );
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
    assert!(!scope.get("out").unwrap().class.contains("Widget"));
}

#[test]
fn printf_arity_counts_supplied_values_without_shifting_format_references() {
    for source in [
        "sprintf('%d %d', , 1L)",
        "sprintf('%d %d', 1L, )",
        "gettextf('%d %d', , 1L)",
    ] {
        let diagnostics = check(source);
        let diagnostic = diagnostics
            .iter()
            .find(|d| d.code == "RY094")
            .unwrap_or_else(|| panic!("{source}: {diagnostics:?}"));
        assert!(
            diagnostic.message.contains("but 1 provided"),
            "{diagnostic:?}"
        );
    }
    // Positional references and star widths have never been part of the
    // count-only contract. One ordinary conversion still has one supplied
    // value despite an extra empty slot; diagnosing that slot is separate.
    for source in [
        "sprintf('%2$d', , 1L)",
        "sprintf('%1$d', , 1L)",
        "sprintf('%*d', , 1L)",
        "sprintf('%d', 1L, )",
        "sprintf('%d %d', 1L, 2L)",
    ] {
        let diagnostics = check(source);
        assert!(
            !diagnostics.iter().any(|d| d.code == "RY094"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn forwarded_dots_can_supply_required_arguments() {
    for source in [
        "f <- function(...) base::attr(..., exact = TRUE)",
        "f <- function(...) gsub(..., ignore.case = TRUE)",
        "f <- function(x, ...) gsub(x = x, ...)",
        "target <- function(x, y) x + y; f <- function(...) target(...)",
        "target <- function(x, y) x + y; f <- function(...) target(tag = ...)",
        "target <- function(x, y) x + y; f <- function(...) target(`...`)",
        "target <- function(xyz, y) xyz + y; f <- function(...) target(xyz = ..., x = 1L)",
        "target <- function(alpha, alpine) alpha + alpine; f <- function(...) target(al = 1L, ...)",
        "target <- function(alpha, alpine) alpha + alpine; f <- function(...) target(`al` = 1L, ...)",
        "target <- function(alpha, alpine) alpha + alpine; f <- function(...) target('al' = 1L, ...)",
        "target <- function(x, y) x + y; f <- function(...) target(..., ...)",
        "target <- function(x, y = 2L) x + y; f <- function(...) target(, ...); f(x = 1L)",
        "target <- function(x = 1L, y) x + y; f <- function(...) target(..., ); f(y = 2L)",
        "target <- function(x, ...) x; f <- function(...) target(..., ); f(1L)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "RY091" && d.code != "RY090"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn forwarding_retains_explicit_named_holes_and_other_argument_checks() {
    for source in [
        "target <- function(x, y) x + y; f <- function(...) target(x = , ...)",
        "f <- function(...) gsub(replacement = , ...)",
        "target <- function(x, y) x + y; f <- function(...) target(x = ..., x = )",
        "target <- function(x, y) x + y; f <- function(...) target(`x` = , ...)",
        "target <- function(x, y) x + y; f <- function(...) target('x' = , ...)",
        "target <- function(x, y) x + y; f <- function(...) target((...))",
        "target <- function(x, y) x + y; f <- function(...) target((`...`))",
        "gsub('a')",
        "f <- function(...) gsub(list(...))",
        "f <- function(...) gsub(quote(...))",
        "gsub('...')",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY091"),
            "{source}: {diagnostics:?}"
        );
    }
    let diagnostics =
        check("target <- function(x, y) x + y; f <- function(...) target(typo = 1L, ...)");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY090"),
        "{diagnostics:?}"
    );
}
