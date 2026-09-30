# oracle: must-warn RY032
install <- function(env = parent.frame()) { delayedAssign("x", { x <- c(1L, 2L); 1L }, assign.env = env, eval.env = env) }
wrapper <- function(env = parent.frame()) list(install)[[1L]](env)
f <- function(x = NULL) { wrapper(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; vector_error <- tryCatch(f(), error = function(e) conditionMessage(e)); stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
