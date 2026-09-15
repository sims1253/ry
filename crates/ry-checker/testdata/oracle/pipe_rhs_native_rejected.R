# oracle: must-flag
# oracle-claim: RY000
# Native-pipe right-hand sides base R's parser rejects before any
# evaluation: an extraction whose object is not the `_` placeholder
# ("function '[' not supported in RHS call of a pipe"), a bare block
# ("function '{' not supported in RHS call of a pipe"), and a bare
# symbol ("The pipe operator requires a function call as RHS").
# R errors while parsing the file; ry must flag each form with RY000.
# Mirrors pipe_rhs_native_accepted.R.
z <- c(10, 20)
a <- 1 |> z[1]
b <- 1 |> { z + 1 }
c <- 1 |> sqrt
