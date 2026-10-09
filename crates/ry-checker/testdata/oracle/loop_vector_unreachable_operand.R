# oracle: must-pass
# A literal FALSE `&&` never evaluates its right operand, so the vector
# retained by an empty loop cannot reach it.
f <- function(xs) {
  x <- c(1L, 2L)
  for (i in xs) x <- 1L
  FALSE && x == 1L
}
stopifnot(identical(f(integer()), FALSE), identical(f(1L), FALSE))
