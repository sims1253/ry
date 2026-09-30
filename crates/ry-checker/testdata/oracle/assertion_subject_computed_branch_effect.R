# oracle: must-warn RY032
f <- function(flag = TRUE, x = 1L) {
  stopifnot(x > 0 && TRUE)
  if (flag) p <- base::assign else p <- function(...) NULL
  (base::identity(p))("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
