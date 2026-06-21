# no-diag
# Named user-defined function as callback. `dbl` is in the FnTable
# with a refined return type (integer). `sapply(1:5, dbl)` simplifies
# to an integer vector of length 5. Using the result arithmetically
# with another integer is well-typed.
dbl <- function(x = 1L) { x * 2L }
v <- sapply(1:5, dbl)
y <- v + 1L
