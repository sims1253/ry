# oracle: must-pass
# purrr's prepend() shape: a call into another package's namespace and base
# I() before the assertion still let it prove `before` scalar or NULL.
prepend <- function(x, values, before = NULL) {
  tools::toTitleCase(I("append(after = 0)"))
  n <- length(x)
  stopifnot(is.null(before) || (before > 0 && before <= n))
  if (is.null(before) || before == 1) {
    c(values, x)
  } else {
    c(x[1:(before - 1)], values, x[before:n])
  }
}
stopifnot(identical(prepend(list(1, 2), "a", before = 2), list(1, "a", 2)))
vector_error <- tryCatch(
  prepend(list(1, 2), "a", before = c(1, 2)),
  error = function(e) conditionMessage(e)
)
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
