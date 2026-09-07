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

exact_forward <- function(p, ...) delayedAssign(x = "held", value = p, ...)
stopifnot(is.null(exact_forward(unbound_capture)))
if (requireNamespace("rlang", quietly = TRUE)) {
  leading_forward <- function(p, ...) rlang::enquo(..., p)
  stopifnot(rlang::is_quosure(leading_forward(unbound_capture)))
  positional_forward <- function(p, ...) rlang::enquo(p, ...)
  partial_forward <- function(p, ...) rlang::enquo(ar = p, ...)
  stopifnot(rlang::is_quosure(positional_forward(unbound_capture)))
  stopifnot(rlang::is_quosure(partial_forward(unbound_capture)))
}
# Unknown named dots can change positional occupancy in a mixed signature.
# Supplying value moves the positional p from value to the normal eval.env.
mixed_forward <- function(p, ...) delayedAssign("held", p, ...)
error <- tryCatch(mixed_forward(stop("normal forwarded control"), value = 1L), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(identical(conditionMessage(error), "normal forwarded control"))
