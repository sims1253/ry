# oracle: known-gap vec_cast's x demand is relational (legal x modes depend on `to`), so RY110 has no static proof and stays silent where R errors
# The founding hms demand behind tidyverse/hms#231: a vacuous-all guard
# (`is.numeric(x) || all(is.na(x))`) followed by `vctrs::vec_cast(x,
# double())` really does fail at runtime for the vacuously admitted
# values -- R pins that below. But the demand is relational: the modes
# `x` may take depend on `to` (`vec_cast(x, NULL)` returns `x`
# unchanged for every mode, `vec_cast(character(), character())` is
# legal), and ry's types have no target-dependent encoding, so the
# overlay stub declares no parameter types and RY110 deliberately stays
# silent on this shape. The final call errors in R while ry says
# nothing: that delta is the recorded capability gap (an intentional
# coverage rollback from the former unconditional numeric demand, which
# false-positived on the NULL- and character-target forms).
is_castable_or_na <- function(x) is.numeric(x) || all(is.na(x))
check_cast_args <- function(args) {
  valid <- map_lgl(args, is_castable_or_na)
  if (!all(valid)) stop("All arguments must be numeric or NA")
  vctrs::vec_cast(args, double())
}
# The guard admits zero-length non-numeric input vacuously.
stopifnot(identical(is_castable_or_na(character()), TRUE))
stopifnot(identical(is_castable_or_na(raw(0)), TRUE))
# The relation the checker cannot express: `to` decides which `x` modes
# are legal. A NULL target returns `x` unchanged; the intended nonempty
# all-NA branch passes a numeric target untouched.
stopifnot(identical(vctrs::vec_cast(character(), NULL), character()))
stopifnot(identical(vctrs::vec_cast(raw(0), NULL), raw(0)))
stopifnot(identical(
  vctrs::vec_cast(c(NA_real_, NA_real_), double()),
  c(NA_real_, NA_real_)
))
# The runtime rejection the gap is about: the numeric target errors on
# exactly the vacuously admitted modes (vctrs 0.7.3 messages recorded
# in the repository's hardening evidence).
stopifnot(inherits(
  tryCatch(
    eval(quote(vctrs::vec_cast(character(), double()))),
    error = identity
  ),
  "error"
))
stopifnot(inherits(
  tryCatch(
    eval(quote(vctrs::vec_cast(raw(0), double()))),
    error = identity
  ),
  "error"
))
# The gap itself: R errors here (vacuous accept, then the numeric-target
# cast rejects), and ry stays silent by contract.
cast_guard <- function(x) {
  stopifnot(is.numeric(x) || all(is.na(x)))
  vctrs::vec_cast(x, double())
}
cast_guard(character())
