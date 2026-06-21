# no-diag
# Math functions from the expanded typeshed: `ceiling`, `floor`,
# `round`, `sqrt`, `log`, `exp` all return double. Using their results
# arithmetically with another double is well-typed.
x <- c(1.5, 2.7, 3.14)
a <- ceiling(x) + 1.0
b <- floor(x) + 1.0
c <- round(x) + 1.0
d <- sqrt(x) + 1.0
e <- exp(x) + 1.0
f <- log(x) + 1.0
g <- log10(x) + 1.0
h <- log2(x) + 1.0
