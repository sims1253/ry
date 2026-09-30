# oracle: must-warn RY032
invisible <- function(value) get("done", envir = parent.frame())
install <- function(env = parent.frame(), done = delayedAssign("x", { x <- c(1L, 2L); 1L }, assign.env = env, eval.env = env)) { invisible(NULL) }
f <- function(x = NULL) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
