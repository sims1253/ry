# no-diag
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
quoted_default <- function(x = rlang::expr(function(arg = !!body_value) arg)) {
  out <- x
  body_value <- 1L
  out
}
