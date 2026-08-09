# no-diag
# `Filter` and `Find` preserve the data type. `Filter(f, x)` returns
# the same type as `x`; `Find(f, x)` returns the element type. Both
# are well-typed when used arithmetically.
even <- function(x) x %% 2 == 0
filtered <- Filter(even, c(1L, 2L, 3L, 4L))
found <- Find(even, c(1L, 2L, 3L))
y <- filtered + 1L
z <- found + 1L
