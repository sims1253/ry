# no-diag
# Ordinary R calls are lazy; a callee can ignore the argument without forcing
# its recursive default.
ignore <- function(z) 1L
f <- function(x = x) ignore(x)
f()
