# no-diag
# `sapply` with an anonymous callback that returns a length-1 atomic.
# The checker models the simplification: `function(x) x + 1` takes a
# double and returns a double, so `sapply(c(1, 2, 3), f)` simplifies to
# a double vector of length 3. Using it arithmetically with another
# double is well-typed: no diagnostics.
v <- sapply(c(1, 2, 3), function(x) x + 1)
y <- v + 2.0
