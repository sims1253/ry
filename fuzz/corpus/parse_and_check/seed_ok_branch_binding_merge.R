# no-diag
# Bindings introduced inside an `if` branch leak to the enclosing scope in R,
# so uses after the `if` must NOT fire RY010. Covers: bind in both branches
# (merged type), bind in a single branch with no `else` (unknown, but visible),
# and reassignment in a branch over a pre-existing binding.
f <- function(a) {
  if (a > 0) { r <- "pos" } else { r <- "neg" }
  paste(r, "!")
}
g <- function(flag) {
  if (flag) { v <- 1 }
  v
}
h <- function(flag) {
  s <- 1L
  if (flag) { s <- "x" }
  paste(s, "!")
}
