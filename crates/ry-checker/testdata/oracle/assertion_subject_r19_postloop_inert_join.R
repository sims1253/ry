# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- function(...) NULL
  for (i in 1:2) put <- function(...) NULL
  (base::identity(put))("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(isTRUE(f()))
