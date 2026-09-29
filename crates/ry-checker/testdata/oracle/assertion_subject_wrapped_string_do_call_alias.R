# oracle: must-warn RY032
put <- base::identity("delayedAssign")
install <- function(env = parent.frame()) {
  base::do.call(args = base::list("x", quote({ x <- c(1L, 2L); 1L }), assign.env = env, eval.env = env), what = put)
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
