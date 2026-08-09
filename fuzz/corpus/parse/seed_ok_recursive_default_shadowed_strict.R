# no-diag
# A bare helper named like a strict builtin may be a lazy user function.
abort <- function(x) 1L
f <- function(x = x) abort(x)
f()
