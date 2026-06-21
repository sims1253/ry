# no-diag
# If-expression without else: R returns NULL when the condition is
# FALSE and there's no else. The join of integer and NULL is integer,
# so arithmetic is well-typed.
x <- if (TRUE) 1L
y <- x + 1
