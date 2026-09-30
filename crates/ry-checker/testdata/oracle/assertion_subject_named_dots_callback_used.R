# oracle: must-warn RY032
run <- function(env, action, ...) {
  action()
  base::do.call(..1, base::list(env))
}
install <- function() run(
  env = parent.frame(),
  action = function() NULL,
  extra = function(env) base::delayedAssign(
    "x", { x <- c(1L, 2L); 1L }, assign.env = env, eval.env = env
  )
)
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
