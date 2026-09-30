# oracle: must-warn RY032
f <- function(flag, x = 1L) {
  stopifnot(x > 0 && TRUE)
  if (flag) put <- base::assign else put <- function(...) NULL
  put("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(TRUE), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
