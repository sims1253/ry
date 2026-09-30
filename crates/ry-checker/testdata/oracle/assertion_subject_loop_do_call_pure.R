# oracle: must-pass
install <- function(env = parent.frame()) {
  p <- function(...) NULL
  for (i in 1:2) {
    base::do.call(p, base::list("x", quote({ x <- c(1L, 2L); 1L }), assign.env = env, eval.env = env))
    p <- function(...) NULL
  }
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
