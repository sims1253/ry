# oracle: must-pass
# Language idioms R accepts, and the premises behind them (#648).
errors <- function(code) inherits(tryCatch(eval(code), error = identity), "error")
a <- 1
b <- 2
q <- quote(a + b)
stopifnot(identical(quote(a) == "a", TRUE))
stopifnot(identical(eval(q) * 2, 6))
stopifnot(identical(quote(1) + 1, 2))
stopifnot(identical(length(q), 3L))
# Ops group dispatch ignores implicit classes, so a classless call errors
# even when `Ops.call` and `Ops.name` exist.
Ops.call <- function(e1, e2) "Ops.call"
Ops.name <- function(e1, e2) "Ops.name"
stopifnot(errors(quote(quote(a + b) * 2)), errors(quote(quote(a) + 1)))
stopifnot(errors(quote(NULL + quote(a))))
# A class attached by any replacement, or a classed operand on either
# side (or in either branch of a union), selects an Ops method.
Ops.foo <- function(e1, e2) "dispatched"
Ops.bar <- function(e1, e2) "dispatched"
classed <- quote(a + b)
oldClass(classed) <- "foo"
stopifnot(identical(classed + 1, "dispatched"))
attributed <- quote(a + b)
attributes(attributed) <- list(class = "foo")
stopifnot(identical(attributed + 1, "dispatched"))
stopifnot(identical(structure(quote(a + b), class = "foo") + 1, "dispatched"))
stopifnot(errors(quote(structure(quote(a), class = "foo"))))
for (p in c(TRUE, FALSE)) {
  rhs <- if (p) structure(1, class = "foo") else structure(2, class = "bar")
  stopifnot(identical(quote(a) + rhs, "dispatched"))
}
# `sapply()` keeps language results in a list.
stopifnot(is.list(sapply(1:2, function(i) quote(a))))
