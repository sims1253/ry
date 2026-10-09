# oracle: must-pass
# Storing an installer without calling it leaves the asserted binding alone.
f <- function(flag, x = 1L) {
  stopifnot(x > 0 && TRUE)
  if (flag) put <- base::assign else put <- function(...) NULL
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f(TRUE), f(FALSE))
