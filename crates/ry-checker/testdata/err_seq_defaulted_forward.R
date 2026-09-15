# expect: RY108, RY108, RY108, RY108, RY108
# A seq.* S3 method that forwards or casts its defaulted `to` without a
# missing(to) check cannot tell a caller-supplied endpoint from the
# default: `seq(hms(1), length.out = 3)` re-enters seq.default with the
# defaulted `to` treated as supplied, so length.out's precedence repeats
# the endpoint (three identical values), and a forwarded `by` on top
# errors with "too many arguments". Founding evidence: tidyverse/hms#231,
# pre-fix R/hms.R:301, fixed by guarding every use with missing(to).

# The audit shape: the unconditional cast forces the defaulted `to` at
# function entry, before any test could distinguish supplied from default.
seq.hms <- function(from = hms(1), to = hms(1), by = NULL, ...) {
  from <- as.numeric(from)
  to <- as.numeric(to)
  hms(seq(from, to, ...))
}

# A direct forward needs no cast to have the same collision: seq.default
# receives the default as an ordinary supplied `to`.
seq.widget <- function(from = 1, to = 9, ...) {
  seq(from, to, ...)
}

# Entering the then-arm of `if (missing(to))` proves the value IS the
# default: forwarding it there forwards the default.
seq.defaulted <- function(from = 1, to = 9, ...) {
  if (missing(to)) {
    seq(from, to, ...)
  } else {
    seq(from, ...)
  }
}

# A statement superassignment leaves the local promise in place, so the
# forward still hands seq.default the default.
seq.super <- function(from = 1, to = 9, ...) {
  to <<- 5
  seq(from, to, ...)
}

# An expression-position guard that does not rebind or diverge leaves the
# defaulted promise readable on the fall-through path.
seq.exprlim <- function(from = 1, to = 9, ...) {
  lim <- if (missing(to)) from + 1
  seq(from, to, ...)
}
