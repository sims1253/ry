# expect: RY000 RY000 RY000
# Native-pipe right-hand sides base R rejects at parse time: an
# extraction whose object is not the `_` placeholder, a bare block,
# and a bare symbol. R's own messages name the offending function;
# ry mirrors them. Valid shapes live in ok_pipe_native_rhs.R.
z <- c(10, 20)
a <- 1 |> z[1]
b <- 1 |> { z + 1 }
c <- 1 |> sqrt
