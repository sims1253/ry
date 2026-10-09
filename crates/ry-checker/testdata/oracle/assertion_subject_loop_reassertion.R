# oracle: must-warn RY032
# Conservative boundary: R succeeds, but a possible installer anywhere in a
# repeated body keeps assertions in that body from proving a scalar.
f <- function(x = 1L) {
  p <- function(...) NULL
  for (i in 1:2) {
    p("x", 1L, envir = environment())
    p <- base::assign
    stopifnot(x > 0 && TRUE)
    if (is.null(x) || x == 1L) TRUE else FALSE
  }
  TRUE
}
stopifnot(isTRUE(f()))
