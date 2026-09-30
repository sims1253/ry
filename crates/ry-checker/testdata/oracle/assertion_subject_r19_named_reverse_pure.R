# oracle: must-pass
f <- function(x = 1L) {
  p <- function(...) NULL
  stopifnot(x > 0 && TRUE)
  p("x", c(1L, 2L), envir = { p <- base::assign; environment() })
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(isTRUE(f()))
