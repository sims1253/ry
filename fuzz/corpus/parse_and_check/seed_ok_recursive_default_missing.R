# no-diag
# missing() inspects whether a promise was supplied without forcing its
# self-referential default.
f <- function(x = x) missing(x)
f()
