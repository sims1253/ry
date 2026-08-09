# no-diag
# `Reduce` with a binary callback. The result type is the element
# type of `x` (here, double). `Reduce(f, c(1.0, 2.0, 3.0))` returns
# double. Using the result arithmetically is well-typed.
v <- Reduce(function(a, b) a + b, c(1.0, 2.0, 3.0))
y <- v + 0.5
