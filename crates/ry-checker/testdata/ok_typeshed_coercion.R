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
# `vctrs::vec_cast(x, to)` is relationally polymorphic: `x` need only be
# castable to `to`, so its stub types `x` demand-only (issue #479) -- the
# numeric demand arms RY110 but asserts no RY092 incompatibility, and
# legal cross-type casts such as `vec_cast("foo", character())` (the
# glue upstream suite's own `expect_identical` shape) stay quiet.
cast_chr <- vctrs::vec_cast("foo", character())
cast_list <- vctrs::vec_cast(list(), list())
