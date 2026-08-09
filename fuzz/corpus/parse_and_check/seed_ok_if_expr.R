# no-diag
# If-expression in assignment position: both branches are integer, so
# the result type is integer. Using it arithmetically is well-typed.
x <- if (TRUE) 1L else 2L
y <- x + 1L
