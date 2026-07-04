# no-diag
# Iterating over a union-typed value must yield the element type of each
# member, not a malformed `union[]`. `for (flag in x)` where `x` is
# `union[logical, integer]` must let `if (flag)` work without RY001.
x <- if (runif(1) > 0.5) TRUE else 1L
for (flag in x) {
  if (flag) print("yes")
}
