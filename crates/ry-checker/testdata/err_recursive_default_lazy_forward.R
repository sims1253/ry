# expect: RY109
# Ordinary R calls are lazy; a callee can ignore the argument without forcing
# its recursive default, so this runs when the argument is missing. The
# default itself can never evaluate, so RY109 warns without a forcing proof.
ignore <- function(z) 1L
f <- function(x = x) ignore(x)
f()
