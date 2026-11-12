# oracle: must-warn RY002
# round() and cumprod() return as many elements as their argument, though
# their stubs return a double/complex union (#899). A two-element result
# used as an `if` condition is an error in R, so ry must report it.
x <- c(1.4, 2.6)
stopifnot(length(round(x)) == 2L, length(cumprod(x)) == 2L)
error <- tryCatch({
  if (round(x) > 1) 1L
  NULL
}, error = identity)
stopifnot(inherits(error, "error"))
