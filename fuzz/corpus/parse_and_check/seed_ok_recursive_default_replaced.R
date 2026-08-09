# no-diag
# Returning before a force and replacing the formal both avoid forcing the
# self-referential default.
f <- function(x = x) {
  return(1L)
  x
}
f()

g <- function(x = x) {
  x <- 1L
  x
}
g()
