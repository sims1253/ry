# oracle: must-warn RY032
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- function(...) NULL
  for (i in 1:2) {
    if (i == 2L) put <- base::assign
  }
  (base::identity(put))("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
