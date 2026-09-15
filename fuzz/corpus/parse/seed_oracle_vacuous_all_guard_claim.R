# oracle: must-pass
# oracle-claim: RY110
# Vacuous quantification: `all()` over a zero-length argument is TRUE on
# every mode and `any()` is FALSE, so a guard of the shape
# `is.numeric(x) || all(is.na(x))` accepts zero-length non-numeric input
# that a downstream mode demand rejects (tidyverse/hms#231, fixed in
# 046414d by guarding the emptiness before the `all()`).
guard <- function(x) is.numeric(x) || all(is.na(x))
fixed <- function(x) is.numeric(x) || (length(x) > 0 && all(is.na(x)))
stopifnot(identical(all(logical(0)), TRUE))
stopifnot(identical(all(character()), TRUE))
stopifnot(identical(any(logical(0)), FALSE))
stopifnot(identical(guard(character()), TRUE))
stopifnot(identical(guard(raw(0)), TRUE))
stopifnot(identical(fixed(character()), FALSE))
# The downstream demand rejects the vacuously accepted values. The
# quote() shape keeps the demos out of the checker's argument checking;
# the errors are runtime truth.
stopifnot(inherits(tryCatch(eval(quote(sqrt(character(0)))), error = identity), "error"))
stopifnot(inherits(tryCatch(eval(quote(round(raw(0)))), error = identity), "error"))
# NULL is excluded from the rule's vacuous modes for the same reason
# RY092 defers on NULL actuals: a dispatch-capable demand may accept it.
stopifnot(inherits(tryCatch(eval(quote(sqrt(NULL))), error = identity), "error"))
# Nonempty all-NA input is the intended branch and passes the demand.
stopifnot(identical(guard(c(NA_real_, NA_real_)), TRUE))
stopifnot(identical(sqrt(c(NA_real_, NA_real_)), c(NA_real_, NA_real_)))
