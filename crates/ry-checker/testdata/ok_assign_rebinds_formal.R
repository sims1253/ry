# no-diag
f <- function(x) {
  base::assign("x", 1L)
  x
}
f()
g <- function(x) {
  delayedAssign("x", 1L)
  x
}
g()
