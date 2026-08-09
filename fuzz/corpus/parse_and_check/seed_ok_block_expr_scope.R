# no-diag
# Braced expressions run sequentially in the current scope and return
# the final statement's value.
x <- {
  y <- 1L
  y + 1L
}
z <- x + 1L
