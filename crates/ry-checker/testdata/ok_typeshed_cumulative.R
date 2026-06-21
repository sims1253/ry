# no-diag
# Cumulative functions: `cumsum`, `cumprod`, `cummax`, `cummin` return
# vectors of the same length as input. Using results arithmetically
# is well-typed.
x <- c(1L, 2L, 3L)
a <- cumsum(x) + 1L
b <- cumprod(x) + 1L
c <- cummax(x) + 1L
d <- cummin(x) + 1L
