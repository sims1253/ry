# no-diag
# Function with explicit return() and implicit trailing return; the
# inferred return type is the join of both branches.
f <- function(x = 1L) {
  if (x > 0) {
    return(x * 2)
  }
  -x
}
y <- f(3)
