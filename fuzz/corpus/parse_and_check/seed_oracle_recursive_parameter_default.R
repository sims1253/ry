# oracle: must-warn RY098
# oracle-claim: RY098
outer <- 1
recursive_default <- function(outer = outer) outer
error <- tryCatch({
  recursive_default()
  NULL
}, error = identity)
stopifnot(inherits(error, "error"))
