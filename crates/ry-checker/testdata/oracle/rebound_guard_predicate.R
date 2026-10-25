# oracle: must-warn RY001
# A project `is.null` that always answers FALSE proves nothing about `x`:
# with the NULL default, `!is.null(x)` is TRUE and `if (NULL > 0)` fails
# with "argument is of length zero".
is.null <- function(x) FALSE
f <- function(x = NULL) {
  if (!is.null(x)) {
    if (x > 0) "positive" else "other"
  }
}
stopifnot(identical(f(1), "positive"))
error <- tryCatch(f(), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(identical(conditionMessage(error), "argument is of length zero"))
