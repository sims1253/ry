# oracle: must-pass
# R never evaluates the arm a literal `if` condition does not take, so the
# unknown arguments and the unbound read below cannot error (issue #703).
f <- function(x, y = 1) x + y

if (FALSE) {
  f(1, z = 2)
}

g <- function(x) {
  if (FALSE) f(x, family = "binomial")
  f(x)
}
stopifnot(identical(g(1), 2))

if (TRUE) {
  f(1)
} else {
  f(1, w = 2)
}

k <- if (FALSE) f(1, q = 2) else 3
stopifnot(identical(k, 3))

if (FALSE) {
  h <- function(x) x + not_bound_anywhere
}
stopifnot(!exists("h", inherits = FALSE))

# The same calls error once the arm is live.
stopifnot(inherits(try(f(1, z = 2), silent = TRUE), "try-error"))
stopifnot(inherits(try(f(1, family = "binomial"), silent = TRUE), "try-error"))
