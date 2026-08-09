# no-diag
standalone_errors <- local({
  helper <- function(x) main(x)
  main <- function(x) helper_impl(x)
  helper_impl <- function(x) x
  main
})
