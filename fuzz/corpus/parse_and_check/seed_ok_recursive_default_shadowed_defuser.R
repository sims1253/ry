# no-diag
# Ordinary R functions are lazy; without interprocedural force proof, a local
# helper named like a defuser remains conservatively silent.
quote <- function(x) x
f <- function(x = x) quote(x)
f()
