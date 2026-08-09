# oracle: must-pass
# oracle-claim: RY010
# Looking up a name with no binding raises an R error.
error <- tryCatch(ry_oracle_name_that_does_not_exist, error = identity)
stopifnot(inherits(error, "error"))
