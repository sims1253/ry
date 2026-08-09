# no-diag
# A self-referential default is harmless when every possible force is in an
# unreachable branch. The rule reports only guaranteed forcing.
f <- function(x = x) if (FALSE) x else 1L
f()
