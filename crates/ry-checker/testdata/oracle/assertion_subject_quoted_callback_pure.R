# oracle: must-pass
run <- function(env, `action` = function(...) NULL) {
  action("x", 1L, assign.env = env, eval.env = env)
}
install <- function() run(act = function(...) NULL, env = parent.frame())
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
