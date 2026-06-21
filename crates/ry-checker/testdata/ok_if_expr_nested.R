# no-diag
# Nested if-expressions: `if (cond1) if (cond2) 1L else 2L else 3L`.
# All branches are integer, so the result type is integer.
x <- if (TRUE) { if (FALSE) 1L else 2L } else 3L
y <- x + 1L
