# oracle: known-gap body<- replaces a helper's body after its definition; ry still summarizes the original literal and keeps the scalar fact (false negative only: a kept fact can only suppress RY032)
noop <- function() NULL
body(noop) <- quote(assign("x", 1:2, envir = parent.frame()))
f <- function(x = 1L) {
  stopifnot(x > 0L && TRUE)
  noop()
  if (is.null(x) || x == 1L) TRUE else FALSE
}
f()
