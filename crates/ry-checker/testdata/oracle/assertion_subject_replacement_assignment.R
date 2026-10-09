# oracle: must-warn RY032
# A replacement or subassignment changes the subject's length after the
# assertion; it drops the scalar fact on every path.
indexed <- function(x = 1L) {
  stopifnot(length(x) == 1L)
  x[2L] <- 2L
  if (is.null(x) || x == 1L) TRUE else FALSE
}
resized <- function(x = 1L) {
  stopifnot(length(x) == 1L)
  length(x) <- 2L
  if (is.null(x) || x == 1L) TRUE else FALSE
}
for (f in list(indexed, resized)) {
  vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
