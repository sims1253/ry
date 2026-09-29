# oracle: must-pass
install <- function() {
  p <- {
    original <- base::delayedAssign
    original <- function(...) NULL
    saved <- original
    saved
  }
  base::do.call(p, base::list())
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
