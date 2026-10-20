# oracle: known-gap a project `if` mask runs inside a call-free helper body and rewrites the caller's x; ry trusts the helper's outward-write summary and keeps the scalar fact (false negative only: a kept fact can only suppress RY032)
`if` <- function(cond, yes, no) assign("x", 1:2, envir = parent.frame(2))
noop <- function() if (TRUE) NULL
f <- function(x = 1L) {
  stopifnot(x > 0L && TRUE)
  noop()
  is.null(x) || x == 1L
}
f()
