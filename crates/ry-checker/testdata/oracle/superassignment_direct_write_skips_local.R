# oracle: must-warn RY001
# A function's own `x <<- v` skips its frame (issue #706 review): it
# rebinds the file-level `x`, while the closure keeps reading the
# function's unchanged local `NULL`, so the closure's condition errors
# exactly as ry's RY001 claims.
x <- FALSE
outer <- function() {
  x <- NULL
  read <- function() if (x) 1
  x <<- TRUE
  read
}
read <- outer()
error <- tryCatch(read(), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(identical(conditionMessage(error), "argument is of length zero"))
stopifnot(identical(x, TRUE))
