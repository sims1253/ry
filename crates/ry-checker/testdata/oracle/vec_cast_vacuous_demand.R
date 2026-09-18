# oracle: must-warn RY110
# oracle-claim: RY110
# The founding hms demand behind tidyverse/hms#231: `vctrs::vec_cast()`
# rejects `x` modes outside the target's own ladder, so a vacuous-all
# guard (`is.numeric(x) || all(is.na(x))`) applied interprocedurally
# through a helper hands the demand values it cannot use. R pins the
# whole premise: the guard admits zero-length non-numeric input, and
# the demand errors on exactly those values. The qualified spelling
# needs no `library()` line, and the quote() shape keeps the error
# demos out of the checker's argument checking.
is_castable_or_na <- function(x) is.numeric(x) || all(is.na(x))
check_cast_args <- function(args) {
  valid <- map_lgl(args, is_castable_or_na)
  if (!all(valid)) stop("All arguments must be numeric or NA")
  vctrs::vec_cast(args, double())
}
stopifnot(identical(is_castable_or_na(character()), TRUE))
stopifnot(identical(is_castable_or_na(raw(0)), TRUE))
# The demand rejects the vacuously accepted values (character and raw
# fall outside every numeric target's ladder) while the intended
# nonempty all-NA branch passes through untouched.
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
stopifnot(identical(
  vctrs::vec_cast(c(NA_real_, NA_real_), double()),
  c(NA_real_, NA_real_)
))
