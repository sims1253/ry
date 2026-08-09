# no-diag
# Base-R pipe (4.1+) desugars to a call to mean with the LHS prepended.
result <- c(1, 2, 3) |> mean()
