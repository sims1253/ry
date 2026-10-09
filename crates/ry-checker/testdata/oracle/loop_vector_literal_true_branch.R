# oracle: must-pass
# `if (TRUE)` always rebinds x, so the vector kept by an empty loop cannot
# reach the later `&&`.
f <- function(xs) {
  x <- c(1L, 2L)
  for (i in xs) x <- 1L
  if (TRUE) x <- 1L
  x == 1L && TRUE
}
stopifnot(f(integer()), f(1L))
