# no-diag
# Multi-assignment: a <- b <- 1L assigns 1L to both a and b. Both
# should resolve to integer. Using them arithmetically is well-typed.
a <- b <- 1L
y <- a + b
