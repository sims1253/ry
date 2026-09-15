# no-diag
# Native-pipe right-hand sides base R can rewrite: plain calls,
# string-headed calls with ordinary names, namespace-qualified calls,
# calls headed by a parenthesized lambda, the named `_` placeholder
# (R 4.2+), and extraction chains rooted at the `_` placeholder
# (R 4.3+). Mirrors the rejected shapes in err_pipe_native_rhs.R; both
# sides of the boundary must stay exact.
z <- list(a = 1L, b = 2L)
plain <- c(1, 2, 3) |> mean()
string_head <- 9 |> "sqrt"()
qualified <- c(1, 2, 3) |> base::mean()
lambda <- 3 |> (\(v) v + 1)()
named_placeholder <- c(1, 2) |> list(x = _)
dollar_chain <- z |> _$a
bracket_chain <- z |> _[["b"]]
mixed_chain <- z |> _[1]$a
