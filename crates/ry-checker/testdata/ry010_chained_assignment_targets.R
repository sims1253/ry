# expect: RY010
# A chained top-level assignment binds every target, not only the outer one:
# `a <- b <- value` evaluates `b <- value` and binds its result to `a`, and
# the `=` and `->` spellings chain the same way. These uses precede the
# definitions, so they resolve through the collected top-level names, as
# uses from another file of the package do. Only `fit.robust` is unbound.
refit <- function(args) {
  list(
    do.call(fit.default, args),
    do.call(fit, args),
    do.call(fit.robust, args),
    fit.default(1, 2),
    DEFAULT_A, DEFAULT_B,
    LIMIT_A, LIMIT_B, LIMIT_C,
    RATE_A, RATE_B,
    SCALE_A, SCALE_B
  )
}

fit <- fit.default <- function(yi, vi) yi / vi
DEFAULT_A <- DEFAULT_B <- 10
LIMIT_A <- LIMIT_B <- LIMIT_C <- 1L
RATE_A = RATE_B = 0.5
2 -> SCALE_B -> SCALE_A
