# oracle: must-flag
bins <- 0L
callee <- function(bins = 30L) if (bins == 1L) 1L else 2L
statement <- function(bins = NULL) {
  bins <<- 1L
  callee(bins)
}
condition <- function(bins = NULL) {
  if ((bins <<- 1L) > 0L) callee(bins)
}
indexed <- function(bins = NULL) {
  bins[1L] <<- 1L
  callee(bins)
}
replacement <- function(bins = NULL) {
  length(bins) <<- 1L
  callee(bins)
}
rightward <- function(bins = NULL) {
  1L ->> bins
  callee(bins)
}
right_condition <- function(bins = NULL) {
  if ((1L ->> bins) > 0L) callee(bins)
}
right_indexed <- function(bins = NULL) {
  1L ->> bins[1L]
  callee(bins)
}
expect_length_error <- function(value) {
  error <- tryCatch(value, error = identity)
  stopifnot(inherits(error, "error"))
  stopifnot(identical(conditionMessage(error), "argument is of length zero"))
}
expect_length_error(statement())
expect_length_error(condition())
expect_length_error(indexed())
expect_length_error(replacement())
expect_length_error(rightward())
expect_length_error(right_condition())
expect_length_error(right_indexed())
stopifnot(identical(bins, 1L))
statement()
