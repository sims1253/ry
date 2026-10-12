# no-diag
f <- function(x, y = {
  x <- 1L
  x
}) {
  y
  x
}
f()
