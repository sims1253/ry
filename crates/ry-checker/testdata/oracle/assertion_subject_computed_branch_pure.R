# oracle: must-pass
f <- function(flag = TRUE, x = 1L) {
  stopifnot(x > 0 && TRUE)
  if (flag) p <- function(...) NULL else p <- function(...) NULL
  (base::identity(p))("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
