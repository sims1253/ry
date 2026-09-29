# oracle: must-pass
run <- function(env, action) {
  action("x", { x <- c(1L, 2L); 1L }, assign.env = env, eval.env = env)
}
install <- function() run(parent.frame(), function(...) NULL)
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
