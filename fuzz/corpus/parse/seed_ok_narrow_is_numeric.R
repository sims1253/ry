# no-diag
# Type narrowing in `if` conditions: `is.numeric(x)` narrows `x` to
# double inside the then branch, so arithmetic is well-typed. The
# `eval(...)` call returns opaque; narrowing refines it inside the if.
x <- eval(parse(text = "1 + 2"))
if (is.numeric(x)) {
  y <- x + 1
  z <- x * 2
}
