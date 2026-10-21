# no-diag
# R never evaluates the arm a literal `if` condition does not take:
# `if (FALSE) { ... }` is R's idiomatic block comment, and the `else` of
# `if (TRUE)` is dead the same way. Nothing in those arms is reported
# (issue #703), including the body of a function defined there.
f <- function(x, y = 1) x + y

if (FALSE) {
  # kept for reference, never run
  f(1, z = 2)
}

g <- function(x) {
  if (FALSE) f(x, family = "binomial")
  if (FALSE) paste0(x, "a")
  f(x)
}

if (TRUE) {
  f(1)
} else {
  f(1, w = 2)
}

k <- if (FALSE) f(1, q = 2) else 3

if (FALSE) {
  h <- function(x) x + not_bound_anywhere
}

# Dead arms still bind, as before: later reads of these names stay quiet.
if (FALSE) {
  only_dead <- 1
  dead_fn <- function() 1
}
only_dead
dead_fn()
