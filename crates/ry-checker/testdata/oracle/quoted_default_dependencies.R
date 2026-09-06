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

pruned_if <- function(x = rlang::expr(function() !!(if (FALSE) body_value else 1L))) {
  out <- x
  body_value <- 1L
  out
}
pruned_else <- function(x = rlang::expr(function() !!(if (TRUE) 1L else body_value))) {
  out <- x
  body_value <- 1L
  out
}
pruned_and <- function(x = rlang::expr(function() !!(FALSE && body_value))) {
  out <- x
  body_value <- 1L
  out
}
pruned_or <- function(x = rlang::expr(function() !!(TRUE || body_value))) {
  out <- x
  body_value <- 1L
  out
}
stopifnot(identical(pruned_if(), quote(function() 1L)))
stopifnot(identical(pruned_else(), quote(function() 1L)))
stopifnot(identical(pruned_and(), quote(function() FALSE)))
stopifnot(identical(pruned_or(), quote(function() TRUE)))

payload_binding <- function(x = rlang::expr(function() !!{ body_value <- 1L; body_value })) {
  out <- x
  body_value <- 1L
  out
}
stopifnot(identical(payload_binding(), quote(function() 1L)))

splice_binding <- function(x = rlang::expr(function() list(!!!{ body_value <- list(1L); body_value }))) {
  out <- x
  body_value <- 1L
  out
}
stopifnot(identical(splice_binding(), quote(function() list(1L))))

pruned_statement <- function(x = rlang::expr(function() !!{ if (FALSE) body_value; 1L })) {
  out <- x
  body_value <- 1L
  out
}
stopifnot(identical(pruned_statement(), quote(function() 1L)))

loop_target <- function(x = rlang::expr(function() !!{ for (i in 1:2) body_value <- 1L; 1L })) {
  out <- x
  body_value <- 1L
  out
}
stopifnot(identical(loop_target(), quote(function() 1L)))
