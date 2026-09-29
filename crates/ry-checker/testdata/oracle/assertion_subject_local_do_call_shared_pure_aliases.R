# oracle: must-pass
install <- function() {
  p0 <- function(...) NULL
  a1 <- if (TRUE) p0 else p0
  b1 <- if (TRUE) p0 else p0
  a2 <- if (TRUE) a1 else b1
  b2 <- if (TRUE) a1 else b1
  a3 <- if (TRUE) a2 else b2
  b3 <- if (TRUE) a2 else b2
  a4 <- if (TRUE) a3 else b3
  base::do.call(a4, base::list())
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
