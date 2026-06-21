# no-diag
# tryCatch with integer main expression and integer error handler:
# the join is integer. Using the result arithmetically is well-typed.
result <- tryCatch({
  1L + 2L
}, error = function(e) NA_integer_)
y <- result + 1L
