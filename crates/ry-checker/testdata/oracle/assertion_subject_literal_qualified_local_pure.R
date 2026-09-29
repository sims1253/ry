# oracle: must-pass
install <- function(env = parent.frame()) {
  `base::delayedAssign` <- function(...) NULL
  p <- `base::delayedAssign`
  base::do.call(p, base::list("x", quote({ x <- c(1L, 2L); 1L }), assign.env = env, eval.env = env))
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
