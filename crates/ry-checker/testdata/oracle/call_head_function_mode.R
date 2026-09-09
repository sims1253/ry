# oracle: must-pass
# R's function-mode call lookup skips non-function bindings in every
# frame: the local value does not shadow the outward function at a call
# head, and the call succeeds.
h <- function() "outer-fn"
g <- function() { h <- 5L; h() }
stopifnot(identical(g(), "outer-fn"))
