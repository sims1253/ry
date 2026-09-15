# expect: RY109
# A bare helper named like a strict builtin may be a lazy user function, so
# this runs. The default itself can never evaluate, so RY109 warns.
abort <- function(x) 1L
f <- function(x = x) abort(x)
f()
