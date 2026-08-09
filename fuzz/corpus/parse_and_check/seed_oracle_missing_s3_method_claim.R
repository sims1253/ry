# oracle: must-pass
# oracle-claim: RY050
# A Summary call on a class with no inherited method errors in R.
x <- structure(list(), class = "ry_oracle_without_summary_method")
error <- tryCatch(Summary(x), error = identity)
stopifnot(inherits(error, "error"))
