# oracle: must-pass
f <- function(x = 1L) {
  p <- function(...) NULL
  for (i in 1:2) {
    stopifnot(x > 0 && TRUE)
    p("x", c(1L, 2L), envir = base::new.env())
    if (is.null(x) || x == 1L) TRUE else FALSE
    p <- base::assign
  }
  TRUE
}
stopifnot(isTRUE(f()))
