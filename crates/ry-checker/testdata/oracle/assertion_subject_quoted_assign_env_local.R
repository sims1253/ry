# oracle: must-pass
install <- function() {
  target <- base::new.env()
  base::delayedAssign("x", 1L, `assign.env` = target, eval.env = target)
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
