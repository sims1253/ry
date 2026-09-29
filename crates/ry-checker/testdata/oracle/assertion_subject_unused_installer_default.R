# oracle: must-pass
install <- function(env = parent.frame(), done = delayedAssign("x", {
  x <- c(1L, 2L)
  1L
}, assign.env = env, eval.env = env)) { base::invisible(NULL) }
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
