# no-diag
# Type narrowing: `!is.null(x)` narrows `x` to non-null inside the
# then branch. Arithmetic on opaque (non-null) is well-typed.
x <- NULL
if (!is.null(x)) {
  y <- length(x)
}
