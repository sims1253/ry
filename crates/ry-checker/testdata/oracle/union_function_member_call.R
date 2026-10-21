# oracle: must-pass
# A call head bound to a NULL-or-function or value-or-function join is
# callable. R calls the closure on the function path. Function lookup
# skips the non-function arm, so only that path can fail, and whether it is
# reached depends on invariants outside the call.
pick <- function(flag) if (flag) NULL else function(x) x + 1
use <- function(x, flag) {
  fn <- pick(flag)
  fn(x)
}
stopifnot(identical(use(1, FALSE), 2))
stopifnot(inherits(tryCatch(use(1, TRUE), error = identity), "error"))

fit <- function(x, grad = NULL) {
  g <- NULL
  if (!is.null(grad)) g <- function(p) grad(p) * 2
  if (!is.null(grad)) g(x) else x
}
stopifnot(identical(fit(3), 3))
stopifnot(identical(fit(3, function(p) p + 1), 8))

value_or_function <- function(flag) {
  g <- 1L
  if (flag) g <- function(x) x + 1L
  g(1L)
}
stopifnot(identical(value_or_function(TRUE), 2L))
stopifnot(inherits(tryCatch(value_or_function(FALSE), error = identity), "error"))
