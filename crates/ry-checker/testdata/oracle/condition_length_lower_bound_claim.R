# oracle: must-pass
# oracle-claim: RY002
# A passing `length(x) > 1L` guard proves the value's length is above
# one on that path alone -- no exact count is needed -- so R rejects
# the value as an `if` condition there ("the condition has length > 1")
# and as a `switch` selector ("EXPR must be a length 1 vector"),
# whatever its mode: environments and functions, whose `length()` is
# not one, get the same switch error. Choosing between a length-2 and
# a length-3 vector keeps the same lower bound through negation.
guarded <- function(x) {
  if (is.logical(x) && length(x) > 1L) {
    if (x) 1L else 0L
  }
}
error <- tryCatch(guarded(c(TRUE, FALSE)), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(grepl("length > 1", conditionMessage(error), fixed = TRUE))

picked <- function(flag) {
  x <- if (flag) c(TRUE, FALSE) else c(TRUE, FALSE, TRUE)
  if (!x) 1L
}
error <- tryCatch(picked(FALSE), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(grepl("length > 1", conditionMessage(error), fixed = TRUE))

error <- tryCatch(switch(c("a", "b"), a = 1L), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(grepl("length 1 vector", conditionMessage(error), fixed = TRUE))

env <- new.env()
env$a <- 1
env$b <- 2
error <- tryCatch(switch(env, a = 1L), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(grepl("length 1 vector", conditionMessage(error), fixed = TRUE))
