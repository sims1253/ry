# expect: RY110, RY110
# Interprocedural vacuous-all guards (issue #479): the guard lives in a
# helper, the demand in the caller. The hms args.R form behind
# tidyverse/hms#231: `is_numeric_or_na` returns the vacuous chain,
# `check_args` applies it elementwise with `map_lgl` and rejects unless
# every element passed, and the continuation hands the values to a
# stub-declared numeric demand. Each diagnostic points at its helper's
# `all()` -- that definition is the defect (hms fixed the helper, not
# its callers), so each helper reports once no matter how many call
# sites validate through it. The `map_lgl` spelling is the faithful hms
# shape (hms imports purrr); the checker reads it
# resolution-independently, so no `library()` line is needed here.
#
# The args.R applier form: `valid` proves every element of `args`
# passed the helper, so the post-rejection continuation is accepted.
is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))
check_args <- function(args) {
  valid <- map_lgl(args, is_numeric_or_na)
  if (!all(valid)) stop("All arguments must be numeric or NA")
  mean(args)
}
# The direct helper-call guard: rejecting on the helper's FALSE makes
# the continuation the accepted path.
is_double_or_na <- function(x) is.double(x) || all(is.na(x))
to_seconds <- function(seconds) {
  if (!is_double_or_na(seconds)) stop("must be numeric or NA")
  sqrt(seconds)
}
# The founding hms demand itself (tidyverse/hms#231): `check_args`
# validates elementwise through the helper, and the continuation hands
# the values to `vctrs::vec_cast()`. That demand is relational -- the
# modes `x` may take depend on `to` (R: `vec_cast(character(), double())`
# errors, but `vec_cast(x, NULL)` returns `x` unchanged), so the
# overlay stub declares no parameter types and this site stays silent
# by contract: a runtime-true rejection the checker cannot prove
# without a target-dependent type. The shape stays here as the
# near-miss control proving the silence is the stub's honesty, not an
# unreachable path.
is_castable_or_na <- function(x) is.numeric(x) || all(is.na(x))
check_cast_args <- function(args) {
  valid <- map_lgl(args, is_castable_or_na)
  if (!all(valid)) stop("All arguments must be numeric or NA")
  vctrs::vec_cast(args, double())
}
