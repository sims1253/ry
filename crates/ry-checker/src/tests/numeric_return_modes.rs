use super::*;

/// The modes of `x` and its union members, for asserting
/// `integer | double` without depending on member order.
fn modes(ty: &RType) -> Vec<Mode> {
    let mut modes = match &ty.members {
        Some(members) if ty.mode == Mode::Union => members.iter().map(|m| m.mode).collect(),
        _ => vec![ty.mode],
    };
    modes.sort_by_key(|mode| format!("{mode:?}"));
    modes
}

fn mode_of(src: &str) -> Vec<Mode> {
    let (diagnostics, scope) = check_with_scope(src);
    assert!(diagnostics.is_empty(), "`{src}`: {diagnostics:?}");
    let x = scope
        .get("x")
        .unwrap_or_else(|| panic!("`{src}`: x should be bound"));
    modes(x)
}

const INTEGER: &[Mode] = &[Mode::Integer];
const DOUBLE: &[Mode] = &[Mode::Double];
const INTEGER_OR_DOUBLE: &[Mode] = &[Mode::Double, Mode::Integer];
const UNKNOWN: &[Mode] = &[Mode::Opaque];

const COMPLEX: &[Mode] = &[Mode::Complex];
const CHARACTER: &[Mode] = &[Mode::Character];

/// `input_mode` returns follow the data arguments' mode, one row per
/// stub and one column per input mode, checked against GNU R 4.6.2
/// (`testdata/oracle/input_mode_and_seq_modes.R`). Cells where R errors
/// leave the result unknown. Before #866, logical input read as double;
/// before r-typeshed#125, `sum(1i)` read as double, `max("a")` as
/// unknown, and `pmax(c(TRUE, FALSE))` as integer (R: logical, but
/// `pmax(TRUE, FALSE)` is integer, so the rule makes no claim).
#[test]
fn input_mode_follows_each_input_mode() {
    let inputs = [
        "c(TRUE, FALSE)",
        "1:2",
        "c(1.5, 2)",
        "complex(real = 1:2, imaginary = 1)",
        "c(\"1\", \"2\")",
    ];
    for (call, row) in [
        ("sum", [INTEGER, INTEGER, DOUBLE, COMPLEX, UNKNOWN]),
        ("cumsum", [INTEGER, INTEGER, DOUBLE, COMPLEX, DOUBLE]),
        ("diff", [INTEGER, INTEGER, DOUBLE, COMPLEX, UNKNOWN]),
        ("abs", [INTEGER, INTEGER, DOUBLE, DOUBLE, UNKNOWN]),
        ("cummax", [INTEGER, INTEGER, DOUBLE, UNKNOWN, DOUBLE]),
        ("cummin", [INTEGER, INTEGER, DOUBLE, UNKNOWN, DOUBLE]),
        ("max", [INTEGER, INTEGER, DOUBLE, UNKNOWN, CHARACTER]),
        ("min", [INTEGER, INTEGER, DOUBLE, UNKNOWN, CHARACTER]),
        ("range", [INTEGER, INTEGER, DOUBLE, UNKNOWN, CHARACTER]),
        ("pmax", [UNKNOWN, INTEGER, DOUBLE, UNKNOWN, CHARACTER]),
        ("pmin", [UNKNOWN, INTEGER, DOUBLE, UNKNOWN, CHARACTER]),
        ("rowsum", [UNKNOWN, INTEGER, DOUBLE, UNKNOWN, UNKNOWN]),
    ] {
        for (input, expected) in inputs.iter().zip(row) {
            let src = if call == "rowsum" {
                format!("x <- rowsum({input}, c(1, 1))")
            } else {
                format!("x <- {call}({input})")
            };
            assert_eq!(mode_of(&src), expected, "`{src}`");
        }
    }
}

/// The data join on R's coercion ladder and skip `na.rm`; empty data
/// makes `max`, `min` and `range` double, as their `-Inf`/`Inf` needs.
#[test]
fn input_mode_joins_the_data_arguments() {
    for (src, expected) in [
        ("x <- sum(1:3, na.rm = TRUE)", INTEGER),
        ("x <- max(1L, 2.5)", DOUBLE),
        ("x <- max(2.5, 1L)", DOUBLE),
        ("x <- max(TRUE, \"a\")", CHARACTER),
        ("x <- sum(TRUE, complex(real = 1, imaginary = 1))", COMPLEX),
        ("x <- pmax(TRUE, 2L)", INTEGER),
        ("x <- max(1.5, \"a\")", CHARACTER),
        ("x <- sum(1L, \"a\")", UNKNOWN),
        // R: `max(integer(0))` is `-Inf`, `max(character(0))` is character.
        ("x <- max(integer(0))", DOUBLE),
        ("x <- range(logical(0))", DOUBLE),
        ("x <- max(seq(length.out = 0))", DOUBLE),
        ("x <- min(seq_len(0))", DOUBLE),
        ("x <- max(character(0))", CHARACTER),
        ("x <- max(integer(0), 1L)", INTEGER),
        ("x <- sum(integer(0))", INTEGER),
        ("x <- max()", DOUBLE),
        // Data that may be empty, or all `NA` under `na.rm = TRUE`.
        ("x <- max(c(TRUE, NA), na.rm = TRUE)", INTEGER_OR_DOUBLE),
        ("x <- max(1:2, na.rm = FALSE)", INTEGER),
        ("x <- max(seq_len(runif(1)))", INTEGER_OR_DOUBLE),
        ("x <- min(c(1.5, NA), na.rm = TRUE)", DOUBLE),
        // ry does not type the `1i` literal, so the call is unknown, not
        // double. An unknown input does not reach the rule.
        ("x <- sum(1+1i)", UNKNOWN),
        ("y <- list(1)[[1]]\nx <- sum(y, NULL)", UNKNOWN),
        // `integer | double` input leaves both open.
        ("n <- runif(1)\nx <- sum(1L, seq(1, n))", INTEGER_OR_DOUBLE),
        ("n <- runif(1)\nx <- sum(seq(1, n), 2.5)", DOUBLE),
    ] {
        assert_eq!(mode_of(src), expected, "`{src}`");
    }
}

/// `seq()` reuses the value-sensitive rule of `:` and `seq.default`'s
/// `is.logint()` test, checked against GNU R 4.6.1. Before #866,
/// `seq(1, 10)` was double and `seq(1, 10, length = 4)` had length 10.
#[test]
fn seq_mode_follows_seq_default() {
    for (src, expected) in [
        ("x <- seq(1, 10)", INTEGER),
        ("x <- seq(from = 1, 10)", INTEGER),
        ("x <- seq(10)", INTEGER),
        ("x <- seq(length.out = 3)", INTEGER),
        ("x <- seq(1L, 10L, by = 2L)", INTEGER),
        ("x <- seq(1.5, 10)", DOUBLE),
        ("x <- seq(1, 10, by = 2)", DOUBLE),
        ("x <- seq(1, 10, by = 1L)", DOUBLE),
        ("x <- seq(1, 10, length.out = 4)", DOUBLE),
        ("x <- seq(1, 10, len = 4)", DOUBLE),
        ("x <- seq.int(1, 10)", INTEGER),
        // R: integer for whole steps, double otherwise.
        ("x <- seq.int(1, 10, by = 2)", INTEGER_OR_DOUBLE),
        ("x <- seq.int(1, 10, length.out = 4)", INTEGER_OR_DOUBLE),
        // R: integer for `from = 1`, double for `from = 1.5`.
        ("n <- runif(1)\nx <- seq(n, 10)", INTEGER_OR_DOUBLE),
        ("n <- runif(1)\nx <- seq(1, n, by = 2)", DOUBLE),
        // R: double for `n = 6`, but `seq(1L, 1, by = 2L)` returns `from`.
        ("n <- runif(1)\nx <- seq(1L, n, by = 2L)", INTEGER_OR_DOUBLE),
    ] {
        assert_eq!(mode_of(src), expected, "`{src}`");
    }
}

/// A partial `length.out` name binds `length.out`, as R matches it.
#[test]
fn seq_partial_length_out_pins_the_length() {
    let (diagnostics, scope) = check_with_scope("x <- seq(1, 10, length = 4)");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("x").unwrap().length, Length::Known(4));
}
