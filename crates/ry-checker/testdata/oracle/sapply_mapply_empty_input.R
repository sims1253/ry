# oracle: must-pass
# An input with unknown static length can be empty, so both higher-order calls
# return an empty list in this witness rather than a definite atomic vector.
x <- integer(1L - 1L)
a <- sapply(x, function(v) 1L, USE.NAMES = FALSE)
b <- mapply(function(v) 1L, x, USE.NAMES = FALSE)
stopifnot(
  is.list(a), is.list(b), length(a) == 0L, length(b) == 0L,
  identical(a$field, NULL), identical(b$field, NULL)
)
