# oracle: must-pass
# A successful assertion excludes the vector path before the later guard.
guarded <- function(before = NULL, n = 3L) {
  stopifnot(is.null(before) || (before > 0 && before <= n))
  if (is.null(before) || before == 1L) "first" else "later"
}
stopifnot(identical(guarded(), "first"))
stopifnot(identical(guarded(1L), "first"))
stopifnot(identical(guarded(2L), "later"))
vector_error <- tryCatch(guarded(c(1L, 2L)), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
