# oracle: must-pass
# A trailing `local` control does not evaluate quoted code; the last
# predicate before it still proves `x` scalar or NULL.
f <- function(x) {
  stopifnot(is.null(x) || length(x) == 1L, local = TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f(1L), f(NULL))
vector_error <- tryCatch(f(c(1L, 2L)), error = function(e) conditionMessage(e))
stopifnot(grepl("is not TRUE", vector_error, fixed = TRUE))
