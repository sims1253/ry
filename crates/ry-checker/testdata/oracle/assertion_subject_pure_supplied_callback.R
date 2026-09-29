# oracle: must-pass
install <- function(env, put) {
  put("x", { x <- c(1L, 2L); 1L }, assign.env = env, eval.env = env)
}
f <- function(x = 1L) {
  install(environment(), function(...) NULL)
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
