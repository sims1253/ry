# oracle: must-pass
# The pinned purrr 481e829 prepend() guard (R/deprec-prepend.R:28-39) without
# its deprecation call. A successful stopifnot rejects a vector `before`
# before the later `||` reads it.
prepend <- function(x, values, before = NULL) {
  n <- length(x)
  stopifnot(is.null(before) || (before > 0 && before <= n))

  if (is.null(before) || before == 1) {
    c(values, x)
  } else {
    c(x[1:(before - 1)], values, x[before:n])
  }
}
stopifnot(identical(prepend(list(1, 2), "a"), list("a", 1, 2)))
stopifnot(identical(prepend(list(1, 2), "a", before = 2), list(1, "a", 2)))
vector_error <- tryCatch(
  prepend(list(1, 2), "a", before = c(1, 2)),
  error = function(e) conditionMessage(e)
)
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
