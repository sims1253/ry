# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  { function(...) NULL }("x", c(1L, 2L), envir = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
