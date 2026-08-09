# no-diag
# Coercion functions: `as.matrix`, `as.data.frame`, `as.list`,
# `as.vector`, `as.factor` all return the expected mode. The variables
# they produce can be used in further operations without diagnostics.
x <- c(1, 2, 3)
m <- as.matrix(x)
df <- as.data.frame(x)
l <- as.list(x)
v <- as.vector(x)
f <- as.factor(x)
