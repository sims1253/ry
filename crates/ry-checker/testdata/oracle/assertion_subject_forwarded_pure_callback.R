# oracle: must-pass
install <- function(env, put) {
  put("x", { x <- c(1L, 2L); 1L }, assign.env = env, eval.env = env)
}
bridge <- function(target, cb) { alias <- cb; install(target, alias) }
f <- function(x = 1L) {
  bridge(environment(), function(...) NULL)
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
