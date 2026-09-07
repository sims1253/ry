# oracle: must-pass
# Capturing one formal does not make the helper's environment controls lazy.
value_capture <- function(p) {
  base::delayedAssign(val = p, x = "held")
  TRUE
}
stopifnot(value_capture(stop("value was forced")))
expr_capture <- function(p) base::substitute(en = list(), ex = p)
stopifnot(identical(expr_capture(unbound_capture), quote(p)))

forces <- function(f) {
  error <- tryCatch(f(stop("normal argument forced")), error = identity)
  stopifnot(inherits(error, "error"))
  stopifnot(identical(conditionMessage(error), "normal argument forced"))
}
forces(function(p) base::delayedAssign(p, 1L))
forces(function(p) base::delayedAssign("held", 1L, eval.env = p))
forces(function(p) base::delayedAssign("held", 1L, assign.env = p))
forces(function(p) base::substitute(expr = x, env = p))
stopifnot(inherits(tryCatch(base::substitute(e = x), error = identity), "error"))

if (requireNamespace("rlang", quietly = TRUE)) {
  dots_capture <- function(p) rlang::enquos(p, .named = FALSE)
  stopifnot(length(dots_capture(unbound_capture)) == 1L)
  forces(function(p) rlang::enquos(.named = p))
  forces(function(p) rlang::enquos(.ignore_empty = p))
}
