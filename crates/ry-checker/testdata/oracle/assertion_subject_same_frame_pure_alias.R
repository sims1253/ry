# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- function(...) NULL
  put("x", 1L, assign.env = environment())
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
