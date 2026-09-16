# oracle: must-warn RY110
# oracle-claim: RY110
# The most idiomatic R reject-guard -- `if (!(G)) return(NULL)` -- must
# count as a divergence: the continuation after the `if` is the
# guard-true path, so a vacuous-all guard (`is.numeric(x) ||
# all(is.na(x))`) arms on the code that follows it. R pins the whole
# premise: the `all()` operand is vacuously TRUE over a zero-length
# value, the early return skips exactly the rejected inputs, and a
# vacuously accepted `character()` therefore reaches `sqrt()` and
# errors there instead of at validation (tidyverse/hms#231's shape).
to_seconds <- function(seconds) {
  if (!(is.numeric(seconds) || all(is.na(seconds)))) {
    return(NULL)
  }
  sqrt(seconds)
}
stopifnot(identical(all(is.na(character())), TRUE))
stopifnot(identical(
  (function(x) is.numeric(x) || all(is.na(x)))(character()),
  TRUE
))
# A vacuously accepted zero-length value skips the return and fails the
# downstream demand; the quote() shape keeps the demo out of the
# checker's argument checking, the error is runtime truth.
stopifnot(inherits(
  tryCatch(eval(quote(to_seconds(character()))), error = identity),
  "error"
))
# Rejected input takes the early return quietly.
stopifnot(identical(to_seconds("a"), NULL))
# Nonempty all-NA input is the intended branch, and numeric input
# passes both guard and demand.
stopifnot(identical(to_seconds(c(NA_real_, NA_real_)), c(NA_real_, NA_real_)))
stopifnot(identical(to_seconds(c(4, 9)), c(2, 3)))
