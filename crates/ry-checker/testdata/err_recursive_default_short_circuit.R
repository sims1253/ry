# expect: RY109, RY109
# The RHS of scalar short-circuit operators is not guaranteed to execute, so
# both functions run. The default itself can never evaluate, so RY109 warns.
f <- function(x = x) FALSE && x
g <- function(x = x) TRUE || x
f()
g()
