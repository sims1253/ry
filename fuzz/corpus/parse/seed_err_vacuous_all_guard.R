# expect: RY092, RY110, RY110, RY110, RY110, RY110
# all() is vacuously TRUE over a zero-length argument, so a validation
# guard of the shape `is.numeric(x) || all(is.na(x))` accepts any
# zero-length non-numeric input. Each site below pairs that guard with a
# downstream mode demand a typeshed stub declares; the local minimal
# form of tidyverse/hms#231, where `hms(seconds = character())` passed
# validation and then failed inside `vec_cast()`. The proven-empty site
# also carries the demand's own RY092: the guard diagnostic names the
# cause, RY092 names the failing call.
#
# The audit shape: the test is an open-world parameter (maybe empty),
# and the accepted branch hands it to a numeric demand.
to_seconds <- function(seconds) {
  if (is.numeric(seconds) || all(is.na(seconds))) {
    sqrt(seconds)
  }
}
# A rejecting if whose continuation is the accepted path.
round_or_stop <- function(x) {
  if (!(is.numeric(x) || all(is.na(x)))) {
    stop("must be numeric or NA")
  }
  round(x)
}
# The same rejecting shape with return(): the most idiomatic R
# reject-guard diverges exactly like stop().
round_or_return <- function(x) {
  if (!(is.numeric(x) || all(is.na(x)))) {
    return(NULL)
  }
  round(x)
}
# stopifnot guards continue into the accepted path as well.
scale_input <- function(x) {
  stopifnot(is.numeric(x) || all(is.na(x)))
  exp(x)
}
# A proven-empty local makes the vacuous accept definite.
empty <- character()
if (is.numeric(empty) || all(is.na(empty))) {
  logged <- log(empty)
}
