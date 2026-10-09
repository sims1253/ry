# oracle: must-pass
# A local operator binding chooses the result without base R's scalar
# coercion, even when the loop carries a length-two vector on entry.
`&&` <- function(x, y) TRUE
f <- function(xs) {
  x <- c(1L, 2L)
  for (i in xs) {
    y <- x
    if (y == 1L && TRUE) i
    x <- 1L
  }
  x
}
stopifnot(identical(do.call(f, list(1L)), 1L))
