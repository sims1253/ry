# oracle: must-pass
# oracle-claim: RY080
# A typed purrr map rejects callback results incompatible with its target mode.
library(purrr)
error <- tryCatch(
  map_dbl(1:3, function(x) paste("n", x)),
  error = identity
)
stopifnot(inherits(error, "error"))
