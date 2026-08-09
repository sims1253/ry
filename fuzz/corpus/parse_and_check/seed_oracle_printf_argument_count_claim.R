# oracle: must-pass
# oracle-claim: RY094
# R rejects a literal sprintf format with more conversions than values.
error <- tryCatch(sprintf("%d %d", 1L), error = identity)
stopifnot(inherits(error, "error"))
