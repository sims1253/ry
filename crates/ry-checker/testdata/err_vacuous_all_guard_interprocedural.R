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
