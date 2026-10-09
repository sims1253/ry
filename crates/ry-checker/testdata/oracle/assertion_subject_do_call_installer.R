# oracle: must-warn RY032
# do.call may run any callable, here an installer; the fact is dropped.
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- base::identity(base::assign)
  base::do.call(put, base::list("x", c(1L, 2L), envir = environment()))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
