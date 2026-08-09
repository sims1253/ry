# oracle: must-pass
# oracle-claim: RY001
# Character conditions are not interpretable as logical values in R.
error <- tryCatch(
  eval(parse(text = 'if ("not logical") 1L')),
  error = identity
)
stopifnot(inherits(error, "error"))
