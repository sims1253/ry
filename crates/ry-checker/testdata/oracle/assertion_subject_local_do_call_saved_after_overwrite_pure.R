# oracle: must-pass
install <- function() {
  p <- base::delayedAssign
  p <- function(...) NULL
  saved <- p
  base::do.call(saved, base::list())
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
