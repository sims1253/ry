# oracle: must-pass
# Native-pipe right-hand sides base R's parser rewrites: plain calls,
# namespace-qualified calls, calls headed by a parenthesized lambda,
# the named `_` placeholder (R 4.2+), and extraction chains rooted at
# the `_` placeholder (R 4.3+). Every statement runs in R; ry must stay
# silent (no Error diagnostics). Mirrors pipe_rhs_native_rejected.R.
z <- list(a = 1L, b = 2L)
stopifnot(
  identical(c(1, 2, 3) |> mean(), 2),
  identical(c(1, 2, 3) |> base::mean(), 2),
  identical(3 |> (\(v) v + 1)(), 4),
  identical(c(1, 2) |> list(x = _), list(x = c(1, 2))),
  identical(z |> _$a, 1L),
  identical(z |> _[["b"]], 2L),
  identical(z |> _[1]$a, 1L)
)
