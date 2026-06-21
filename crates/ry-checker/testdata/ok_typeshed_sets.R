# no-diag
# Set operations and utility functions: `intersect`, `union`, `setdiff`,
# `duplicated`, `unique`, `sort`, `order`, `rank`, `match`, `which`
# all have well-typed results when applied to atomic vectors.
x <- c(3L, 1L, 2L, 1L)
a <- intersect(x, c(1L, 2L))
b <- union(x, c(4L, 5L))
c <- setdiff(x, c(1L))
d <- duplicated(x)
e <- unique(x)
f <- sort(x)
g <- order(x)
h <- rank(x)
i <- match(x, c(1L, 2L))
j <- which(x > 1L)
