# oracle: must-pass
f <- function(x = 1L) {
  p <- base::assign
  stopifnot(x > 0 && TRUE)
  p("x", c(1L, 2L), envir = { p <- function(...) NULL; base::new.env() })
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(isTRUE(f()))
