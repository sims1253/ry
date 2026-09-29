# oracle: must-pass
f <- function(flag, x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- base::assign
  if (flag) put <- function(...) NULL else put <- function(...) NULL
  put("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f(TRUE), f(FALSE))
