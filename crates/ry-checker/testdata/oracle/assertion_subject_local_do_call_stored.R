# oracle: must-pass
install <- function(env = parent.frame()) {
  p <- base::delayedAssign
  base::invisible(p)
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
