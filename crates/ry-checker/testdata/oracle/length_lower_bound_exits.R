# oracle: must-warn RY001
# A `length(x) > 1L` bound survives a `return()` exit, a false `||`
# before an early exit, and a join with another long value (#838). Each
# guarded value has two or more elements where it is used, so R rejects
# it as an `if` condition. The controls run.
returned <- function(x) {
  if (length(x) <= 1L) return(NULL)
  if (x) 1L
}
disjunction <- function(x, y) {
  if (length(x) <= 1L || is.null(y)) stop("short")
  if (x) 1L
}
joined <- function(x, flag) {
  if (length(x) > 1L) {
    y <- if (flag) x else c(TRUE, FALSE)
    if (y) 1L
  }
}
for (call in list(
  quote(returned(c(TRUE, FALSE))),
  quote(disjunction(c(TRUE, FALSE), 1)),
  quote(joined(c(TRUE, FALSE), TRUE)),
  quote(joined(c(TRUE, FALSE), FALSE))
)) {
  error <- tryCatch(eval(call), error = identity)
  stopifnot(inherits(error, "error"))
  stopifnot(grepl("length > 1", conditionMessage(error), fixed = TRUE))
}

masked_return <- function(x) {
  return <- function(value) value
  if (length(x) <= 1L) return(NULL)
  if (x) 1L
}
scalar_join <- function(x, flag) {
  if (length(x) > 1L) {
    y <- if (flag) x else TRUE
    if (y) 1L
  }
}
stopifnot(identical(masked_return(TRUE), 1L))
stopifnot(identical(scalar_join(c(TRUE, FALSE), FALSE), 1L))
