# oracle: must-warn RY032
# A function literal passed to a base higher-order function may run and
# install a caller binding; the scalar fact is dropped.
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  env <- environment()
  lapply(1L, function(i) assign("x", c(1L, 2L), envir = env))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
