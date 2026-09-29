# oracle: must-pass
install <- function(env, put) {
  base::invisible(NULL)
}
f <- function(x = 1L) {
  install(environment(), delayedAssign)
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
