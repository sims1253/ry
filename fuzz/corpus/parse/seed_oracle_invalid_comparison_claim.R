# oracle: must-pass
# oracle-claim: RY030
# R does not define ordering between a closure and an atomic number.
error <- tryCatch(
  eval(parse(text = '(function() 1L) > 1L')),
  error = identity
)
stopifnot(inherits(error, "error"))
