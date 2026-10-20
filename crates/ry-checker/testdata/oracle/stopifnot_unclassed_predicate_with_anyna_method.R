# oracle: must-pass
# A project anyNA method cannot run for a predicate value without a class
# attribute, so `stopifnot(p)` with a local `p <- TRUE` keeps the scalar
# fact (contrast assertion_subject_stopifnot_classed_predicate.R).
anyNA.probe <- function(x, recursive = FALSE) stop("dispatched")
f <- function(x = 1L) {
  p <- TRUE
  stopifnot(x > 0L && TRUE)
  stopifnot(p)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(isTRUE(f()), identical(f(2L), FALSE))
vector_call <- tryCatch(f(1:2), error = function(e) deparse(conditionCall(e)))
stopifnot(identical(vector_call, "x > 0L && TRUE"))
