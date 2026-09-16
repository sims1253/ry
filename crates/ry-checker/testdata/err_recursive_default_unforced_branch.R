# expect: RY109
# A self-referential default is harmless at runtime when every possible force
# is in an unreachable branch. RY098 reports only guaranteed forcing; RY109
# still warns, because the default itself can never evaluate.
f <- function(x = x) if (FALSE) x else 1L
f()
