# no-diag
# `x %in% table` returns a logical vector of length(x); the RHS length is
# irrelevant. A length-1 `x` matched against a length-2 literal stays
# length-1 logical, so it is a valid `if` condition (no RY002) and a valid
# `&&` operand (no RY032).
x <- "a"
if (x %in% c("a", "b")) print(1)
if (is.character(x) && x %in% c("a", "b")) print(2)
