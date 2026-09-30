# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- function(...) NULL
  base::do.call(put, base::list("x", c(1L, 2L), envir = environment()))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
