# oracle: must-pass
quoted <- function(x = base::quote(body_value)) {
  out <- x
  body_value <- 1L
  out
}
named <- function(x = base:::quote(expr = body_value)) {
  out <- x
  body_value <- 1L
  out
}
substituted <- function(x = base::substitute(body_value)) {
  out <- x
  body_value <- 1L
  out
}
expressions <- function(x = base::expression(body_value, body_value + 1L)) {
  out <- x
  body_value <- 1L
  out
}
captured <- function(x = rlang::expr(body_value)) {
  out <- x
  body_value <- 1L
  out
}
injected_quote <- function(x = rlang::expr(!!base::quote(body_value))) {
  out <- x
  body_value <- 1L
  out
}
for (fn in list(quoted, named, substituted, captured, injected_quote)) {
  stopifnot(identical(fn(), quote(body_value)))
}
stopifnot(identical(expressions(), expression(body_value, body_value + 1L)))

quoted_default <- function(x = rlang::expr(function(arg = !!body_value) arg)) {
  out <- x
  body_value <- 1L
  out
}
stopifnot(identical(quoted_default(), quote(function(arg = !!body_value) arg)))

literal_bangs <- function(x = base::quote(function() !!body_value)) {
  out <- x
  body_value <- 1L
  out
}
stopifnot(identical(literal_bangs(), quote(function() !!body_value)))
