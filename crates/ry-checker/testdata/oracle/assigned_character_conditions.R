# oracle: must-warn RY001
bad <- function() {
  value <- "hello"
  alias <- value
  if (alias) 1L else 2L
}
good <- function() {
  value <- "TRUE"
  if (value) 1L else 2L
}
rebound <- function() {
  value <- "hello"
  value <- "False"
  if (value) 1L else 2L
}
stopifnot(good() == 1L, rebound() == 2L)
error <- tryCatch(bad(), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(identical(conditionMessage(error), "argument is not interpretable as logical"))
