# oracle: must-pass
f <- function(put, x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- function(...) NULL
  put("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f(base::assign))
