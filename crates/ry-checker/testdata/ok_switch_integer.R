# no-diag
# switch() with all-integer alternatives: the result type is integer.
# Using it arithmetically is well-typed.
x <- switch("b", a = 1L, b = 2L, c = 3L)
y <- x + 1L
