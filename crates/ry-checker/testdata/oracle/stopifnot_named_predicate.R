# oracle: must-pass
# An arbitrary name in `...` remains a checked assertion in base stopifnot.
f <- function(x) {
  stopifnot(named_condition = is.null(x) || length(x) == 1L)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(identical(do.call(f, list(1L)), TRUE))
vector_error <- tryCatch(do.call(f, list(c(1L, 2L))), error = function(e) conditionMessage(e))
stopifnot(grepl("named_condition", vector_error, fixed = TRUE))
