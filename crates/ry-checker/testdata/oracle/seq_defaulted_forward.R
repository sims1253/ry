# oracle: must-warn RY108
# tidyverse/hms#231: seq.hms cast and forwarded its defaulted `to`
# unconditionally, so dispatch like `seq(hms(1), length.out = 3)` re-entered
# seq.default with the default treated as a supplied endpoint. R shows both
# symptoms: three identical values (length.out wins over the forwarded
# default), and "too many arguments" once `by` is forwarded too. The
# missing()-guarded form produces the intended sequences on the same calls.
hms <- function(x) structure(x, class = c("hms", "difftime"), units = "secs")

seq.hms <- function(from = hms(1), to = hms(1), by = NULL, ...) {
  from <- as.numeric(from)
  to <- as.numeric(to)
  if (!is.null(by)) {
    by <- as.numeric(by)
    return(hms(seq(from, to, by, ...)))
  }
  hms(seq(from, to, ...))
}

# The defaulted `to` is indistinguishable from a supplied one once forced:
# length.out takes precedence, so the default endpoint repeats.
stopifnot(identical(as.numeric(seq(hms(1), length.out = 3)), c(1, 1, 1)))
# ... and with `by` forwarded too, seq.default rejects the combination.
stopifnot(identical(
  tryCatch(seq(hms(1), by = hms(2), length.out = 3), error = function(e) "error"),
  "error"
))

seq.guarded <- function(from = hms(1), to = hms(1), by = NULL, ...) {
  from <- as.numeric(from)
  if (!is.null(by)) {
    by <- as.numeric(by)
    if (missing(to)) {
      return(hms(seq(from, by = by, ...)))
    }
    return(hms(seq(from, as.numeric(to), by, ...)))
  }
  if (missing(to)) {
    return(hms(seq(from, ...)))
  }
  hms(seq(from, as.numeric(to), ...))
}
# The guarded method answers the same dispatches the way the upstream fix
# does (hms tests/testthat/test-sequence.R).
stopifnot(identical(as.numeric(seq.guarded(hms(1), length.out = 3)), c(1, 2, 3)))
stopifnot(identical(
  as.numeric(seq.guarded(hms(1), by = hms(2), length.out = 3)),
  c(1, 3, 5)
))
