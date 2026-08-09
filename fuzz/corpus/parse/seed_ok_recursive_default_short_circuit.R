# no-diag
# The RHS of scalar short-circuit operators is not guaranteed to execute.
f <- function(x = x) FALSE && x
g <- function(x = x) TRUE || x
f()
g()
