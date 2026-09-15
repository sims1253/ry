# expect: RY109, RY109
# Returning before a force and replacing the formal both avoid forcing the
# self-referential default, so both functions run. The default itself can
# never evaluate (the second one is dead the moment `x <- 1L` runs), so
# RY109 warns without a forcing proof.
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
