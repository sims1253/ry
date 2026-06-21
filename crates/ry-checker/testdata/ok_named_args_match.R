# no-diag
# Named arguments matched to parameters: rep(x, times) called with
# out-of-order named args. The result mode follows the x parameter
# (double), not the first positional arg.
v <- rep(times = 3L, x = c(1.5, 2.5))
y <- v + 1.0
