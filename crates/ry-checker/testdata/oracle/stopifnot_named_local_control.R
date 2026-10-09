# oracle: must-warn RY032
# `local` controls stopifnot's evaluation environment; FALSE does not fail
# the assertion. It therefore cannot validate x for the later `||`.
f <- function(x) {
  stopifnot(local = is.null(x) || length(x) == 1L)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(identical(do.call(f, list(1L)), TRUE))
vector_error <- tryCatch(do.call(f, list(c(1L, 2L))), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
