# oracle: must-warn RY032
put <- "delayedAssign"
install <- function(env = parent.frame()) {
  base::do.call(put, base::list("x", quote({ x <- c(1L, 2L); 1L }), assign.env = env, eval.env = env))
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
