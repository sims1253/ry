# expect: RY109
# Ordinary R functions are lazy; without interprocedural force proof, a local
# helper named like a defuser may never force its argument, so this runs.
# The default itself can never evaluate, so RY109 warns.
quote <- function(x) x
f <- function(x = x) quote(x)
f()
