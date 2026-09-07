# oracle: must-pass
# Fold initializers describe the first invocation, not later accumulators.
stopifnot(identical(
  Reduce(function(keep, cmp) keep | startsWith('cc main.c', cmp), c('cc', 'c++'), FALSE),
  TRUE
))
stopifnot(identical(
  Reduce(x = c('cc', 'c++'), init = FALSE,
         f = function(cmp, keep) keep | startsWith('cc main.c', cmp), right = TRUE),
  TRUE
))
stopifnot(identical(
  Reduce(function(a, b) if (is.character(a)) 1L else a + 1L, c('a', 'b', 'c')),
  2L
))
stopifnot(identical(Reduce(function(a, b) 'changed', 1:3, FALSE), 'changed'))
stopifnot(identical(Reduce(function(a, b) TRUE, character(), list(1L)), list(1L)))
stopifnot(identical(Reduce(function(a, b) b | TRUE, 'a'), 'a'))
stopifnot(identical(Reduce(function(a, b) b | TRUE, 'a', init = ), 'a'))
stopifnot(identical(Reduce(function(a, b) b | TRUE, character(), FALSE), FALSE))
stopifnot(identical(Reduce(function(a, b) TRUE, c('a', 'b'), accumulate = TRUE), c('a', 'TRUE')))
for (right in c(FALSE, TRUE)) {
  callback <- if (right) function(element, accumulator) accumulator | startsWith('cc main.c', element)
              else function(accumulator, element) accumulator | startsWith('cc main.c', element)
  stopifnot(identical(Reduce(callback, c('cc', 'c++'), FALSE, right = right), TRUE))
}
stopifnot(inherits(try(eval(quote(Reduce(function(a, b) b | TRUE, c('a', 'b')))), silent = TRUE), 'try-error'))
stopifnot(inherits(try(eval(quote(Reduce(function(a, b) a | TRUE, c('a', 'b'), right = TRUE))), silent = TRUE), 'try-error'))
if (requireNamespace('purrr', quietly = TRUE)) {
  stopifnot(identical(purrr::reduce(c('a', 'b'), function(a, b) a | TRUE, .init = FALSE), TRUE))
  stopifnot(identical(purrr::reduce(c('a', 'b'), function(a, b) b | TRUE, .init = FALSE, .dir = 'backward'), TRUE))
  stopifnot(identical(purrr::reduce('a', function(a, b) TRUE), 'a'))
  stopifnot(inherits(try(eval(quote(purrr::reduce(c('a', 'b'), function(a, b) b | TRUE))), silent = TRUE), 'try-error'))
  stopifnot(inherits(try(eval(quote(purrr::reduce(c('a', 'b'), function(a, b) a | TRUE, .dir = 'backward'))), silent = TRUE), 'try-error'))
}
