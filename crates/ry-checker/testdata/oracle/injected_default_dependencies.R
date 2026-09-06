# oracle: must-warn RY098
# oracle-claim: RY098
ordinary <- function(x = base::identity(body_value)) {
  out <- x
  body_value <- 1L
  out
}
substitution_environment <- function(x = base::substitute(other, env = body_value)) {
  out <- x
  body_value <- list()
  out
}
injected <- function(x = rlang::expr(!!body_value)) {
  out <- x
  body_value <- 1L
  out
}
spliced <- function(x = rlang::expr(list(!!!body_value))) {
  out <- x
  body_value <- list()
  out
}
embraced <- function(x = rlang::expr({{ body_value }})) {
  out <- x
  body_value <- 1L
  out
}
quoted_function <- function(x = rlang::expr(function() !!body_value)) {
  out <- x
  body_value <- 1L
  out
}
bquoted <- function(x = base::bquote(.(body_value))) {
  out <- x
  body_value <- 1L
  out
}
bquote_spliced <- function(x = base::bquote(list(..(body_value)), splice = TRUE)) {
  out <- x
  body_value <- list()
  out
}
for (fn in list(ordinary, substitution_environment, injected, spliced, embraced,
                quoted_function, bquoted, bquote_spliced)) {
  result <- tryCatch(fn(), error = function(error) error)
  stopifnot(inherits(result, "error"))
}
