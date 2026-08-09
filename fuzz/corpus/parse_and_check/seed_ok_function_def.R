# no-diag
# Function definitions with typed-by-default params should not trip.
f <- function(a = 1L, b = 2.0) {
  a + b
}
g <- f(1L, 2.0)
