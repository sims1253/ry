use super::*;

fn mixed_sign_errors(source: &str) -> usize {
    check(source).iter().filter(|d| d.code == "RY126").count()
}

#[test]
fn proven_straddling_sequences_fire() {
    for source in [
        "x <- 1:10\nx[1:-1]\n",
        "x <- 1:10\nx[-2:3]\n",
        "x <- 1:10\nx[seq(2, -3, by = -2)]\n",
        "x <- 1:10\nx[base::seq(1, -1)]\n",
        "x <- 1:10\nx[seq(to = -1, from = 1)]\n",
        "x <- c('a', 'b')\nx[1.0:-2.0]\n",
        "lst <- list(1, 2)\nlst[1:-1]\n",
        "x <- 1:10\nn <- -2L\nx[seq(1, n)]\n",
        "x <- 1:10\nx[[seq(1, -1)]] <- 0L\n",
        "f <- function() {\n v <- 1:10\n v[1:-1]\n}\n",
        "f <- function() {\n v <- 1:10\n v[base::seq(-1, 2)]\n}\n",
    ] {
        assert_eq!(mixed_sign_errors(source), 1, "{source}");
    }
}

#[test]
fn one_sided_and_computed_sequences_stay_silent() {
    for source in [
        // Legitimate descending / one-sided idioms.
        "x <- 1:10\nx[3:1]\n",
        "x <- 1:10\nx[-3:-1]\n",
        "x <- 1:10\nx[0:-1]\n",
        "x <- 1:10\nx[1:0]\n",
        // `by` lands on 0 and stops; no negative term exists.
        "x <- 1:10\nx[seq(2, -1, by = -2)]\n",
        // seq's own wrong-sign error is not RY126's claim.
        "x <- 1:10\nx[seq(1, -1, by = 2)]\n",
        // `length.out` and duplicated-formal forms are outside the
        // proven shape set. (A single negative argument is not:
        // `seq(-1)` runs `1:-1` via the default endpoint.)
        "x <- 1:10\nx[seq(1, -1, length.out = 2)]\n",
        "x <- 1:10\nx[seq(1, -1, from = 2)]\n",
        // Fractional endpoints may truncate to zeros.
        "x <- 1:10\nx[seq(0.5, -0.5, by = -1)]\n",
        // Computed endpoints (#355's remainder) are not proven.
        "x <- 1:10\nf <- function(n) x[1:(n - 1)]\n",
        "x <- 1:10\nf <- function(n) x[seq2(1, n - 1)]\n",
        // Endpoint bindings that later writes or joins disturb.
        "x <- 1:10\nn <- -1L\nn <- 5L\nx[1:n]\n",
        "x <- 1:10\nf <- function(cond) {\n n <- if (cond) -1L else -2L\n x[1:n]\n}\n",
        // A parameter default describes one call shape only.
        "x <- 1:10\nf <- function(k = -1L) x[1:k]\n",
        // Identifier endpoints in a later subscript are read against
        // pre-inference facts, so they do not count there.
        "x <- matrix(1:20, 4, 5)\nn <- -1L\ny <- 1:10\ny[1, 1:n]\n",
        // Non-index uses iterate over the sequence happily.
        "for (i in 1:-1) print(i)\n",
        // seq_len / seq_along cannot produce a negative term.
        "x <- 1:10\nx[seq_len(3)]\nx[seq_along(x)]\n",
    ] {
        assert_eq!(mixed_sign_errors(source), 0, "{source}");
    }
}

#[test]
fn unproven_receivers_and_masked_definitions_stay_silent() {
    for source in [
        // Unknown and classed receivers may dispatch to `[` methods.
        "f <- function(v) v[1:-1]\n",
        "fct <- factor(c('a', 'b'))\nfct[1:-1]\n",
        "df <- data.frame(a = 1:3)\ndf[1:-1]\n",
        "nothing <- NULL\nnothing[1:-1]\n",
        // `[[` into a list can consume a negative step.
        "lst <- list(list(1))\nlst[[1:-1]]\n",
        // Masked sequence definitions.
        "f <- function() {\n seq <- function(a, b) 1L\n y <- c(1, 2)\n y[seq(1, -1)]\n}\n",
        "f <- function() {\n `:` <- function(a, b) 1L\n y <- c(1, 2)\n y[1:-1]\n}\n",
        "f <- function() {\n seq2 <- function(a, b) a:b\n y <- c(1, 2)\n y[seq2(-1, 2)]\n}\n",
        // A rebound unary `-` makes `-1` an unknown value.
        "f <- function() {\n `-` <- function(e1) 3L\n y <- c(1, 2)\n y[1:-1]\n}\n",
        // A non-function `:` binding trips only `operator_rebound`
        // (`has_explicit_operator_mask` looks for callable masks),
        // pinning that gate independently.
        "f <- function() {\n `:` <- 3L\n y <- c(1, 2)\n y[1:-1]\n}\n",
        // A `seq` call R rejects before building a sequence
        // ("too many arguments" once by/length.out/along.with
        // collide) does not match one-to-one, so the mixed-sign
        // claim never forms.
        "y <- c(1, 2)\ny[seq(1, -1, 2, 5, 9, 7)]\n",
        // An extra named argument is only warned away by
        // seq.default's chkDots and the sequence still forms, so
        // silence here is a conservative miss of the one-to-one
        // gate, not a rejection.
        "y <- c(1, 2)\ny[seq(1, -1, foo = 3)]\n",
    ] {
        assert_eq!(mixed_sign_errors(source), 0, "{source}");
    }
}

#[test]
fn seq2_only_fires_when_it_ascends_across_zero() {
    // rlang::seq2(1, -1) is integer(0), not c(1, 0, -1).
    assert_eq!(mixed_sign_errors("x <- 1:10\nx[rlang::seq2(1, -1)]\n"), 0);
    assert_eq!(mixed_sign_errors("x <- 1:10\nx[rlang::seq2(-1, 2)]\n"), 1);
}

#[test]
fn assignment_targets_fire_once() {
    assert_eq!(mixed_sign_errors("x <- 1:10\nx[1:-1] <- 0L\n"), 1);
    let source = "x <- 'a'\nx[seq(1, -1)] <- 'b'\n";
    assert_eq!(mixed_sign_errors(source), 1, "{source}");
}

#[test]
fn named_atomic_receivers_and_seq_endpoint_defaults() {
    // A names schema does not change atomic subsetting.
    assert_eq!(mixed_sign_errors("x <- c(a = 1, b = 2); x[1:-1]"), 1);
    // seq(to = -1) runs 1:-1 via seq.default's documented default.
    assert_eq!(mixed_sign_errors("x <- 1:9; x[seq(to = -1)]"), 1);
    assert_eq!(mixed_sign_errors("x <- 1:9; x[seq.int(to = -1)]"), 1);
    assert_eq!(mixed_sign_errors("x <- 1:9; x[seq(-2)]"), 1);
    // One-sided sequences stay silent.
    assert_eq!(mixed_sign_errors("x <- 1:9; x[seq(to = 2)]"), 0);
    assert_eq!(mixed_sign_errors("x <- 1:9; x[seq(3)]"), 0);
}

#[test]
fn seq2_requires_both_endpoints() {
    // rlang::seq2 has no defaults: a missing endpoint errors in R
    // instead of producing a straddling sequence.
    assert_eq!(mixed_sign_errors("x <- 1:9\nx[rlang::seq2(-1)]\n"), 0);
    assert_eq!(mixed_sign_errors("x <- 1:9\nx[rlang::seq2(to = 1)]\n"), 0);
}
