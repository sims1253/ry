# oracle: must-flag
# oracle-claim: RY061
# `x[[1]]` is the first position, the named `a`, not the first unnamed
# element, so `$` on it errors: $ operator is invalid for atomic vectors.
x <- list(a = "s", list(b = 2))
x[[1]]$b
