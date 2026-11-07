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

/// `double_or_int` returns follow every data argument, checked against
/// GNU R 4.6.1 (`testdata/oracle/double_or_int_and_seq_modes.R`). Before
/// #866, logical input and anything `...` absorbed ahead of `na.rm`
/// (`sum`) read as double, and `max("a", "b")` claimed double.
#[test]
fn double_or_int_follows_the_data_arguments() {
    for (src, expected) in [
        ("x <- sum(c(TRUE, FALSE))", INTEGER),
        ("x <- sum(1:3)", INTEGER),
        ("x <- sum(1:3, na.rm = TRUE)", INTEGER),
        ("x <- abs(TRUE)", INTEGER),
        ("x <- max(TRUE)", INTEGER),
        ("x <- cumsum(c(TRUE, FALSE))", INTEGER),
        ("x <- max(c(TRUE, NA), na.rm = TRUE)", INTEGER),
        ("x <- sum(1.5)", DOUBLE),
        ("x <- max(1L, 2.5)", DOUBLE),
        ("x <- max(2.5, 1L)", DOUBLE),
        // R: character for max/min/range, an error for sum and abs.
        ("x <- max(\"a\", \"b\")", UNKNOWN),
        ("x <- min(\"a\")", UNKNOWN),
        ("x <- range(c(\"a\", \"b\"))", UNKNOWN),
        ("x <- max(1.5, \"a\")", UNKNOWN),
        // `integer | double` input leaves both open.
        ("n <- runif(1)\nx <- sum(1L, seq(1, n))", INTEGER_OR_DOUBLE),
        ("n <- runif(1)\nx <- sum(seq(1, n), 2.5)", DOUBLE),
    ] {
        assert_eq!(mode_of(src), expected, "`{src}`");
    }
}

/// Complex input is complex for `sum` but double for `abs`, so a value
/// typed complex leaves the result mode unknown rather than double.
#[test]
fn double_or_int_leaves_complex_input_unknown() {
    let prefix = "z <- complex(real = 1, imaginary = 1)\n";
    for call in ["sum(z)", "abs(z)", "max(z, 1L)"] {
        assert_eq!(
            mode_of(&format!("{prefix}x <- {call}")),
            UNKNOWN,
            "`{call}`"
        );
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
