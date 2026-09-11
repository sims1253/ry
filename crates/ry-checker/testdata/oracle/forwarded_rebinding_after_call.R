# oracle: must-warn RY001
callee <- function(bins = 30L) if (bins == 1L) 1L else 2L
caller <- function(bins = NULL) {
  identity(callee(bins))
  bins <- 1L
}
error <- tryCatch(caller(), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(identical(conditionMessage(error), "argument is of length zero"))
