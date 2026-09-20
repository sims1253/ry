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
# castable to `to` (`vec_cast(x, NULL)` returns `x` unchanged), so its
# overlay stub pins R's formals without parameter types -- no RY092
# assertion and no RY110 demand -- and every legal cross-type cast such
# as `vec_cast("foo", character())` (the glue upstream suite's own
# `expect_identical` shape) stays quiet.
cast_chr <- vctrs::vec_cast("foo", character())
cast_list <- vctrs::vec_cast(list(), list())
cast_null <- vctrs::vec_cast(character(), NULL)
