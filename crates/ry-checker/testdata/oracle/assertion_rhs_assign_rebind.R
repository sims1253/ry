# oracle: must-warn RY032
# The assertion's first comparison sees a scalar. Its later RHS replaces x
# before the next condition; the successful assertion cannot validate x.
f <- function(x) {
  stopifnot(is.null(x) || (x > 0 && { assign("x", c(1L, 2L)); TRUE }))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(identical(do.call(f, list(NULL)), TRUE))
vector_error <- tryCatch(do.call(f, list(1L)), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
