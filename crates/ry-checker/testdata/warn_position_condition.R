# expect: RY003
# `Position()` returns `nomatch` — `NA_integer_` by default — when
# nothing matches, so `if (Position(...))` is TRUE-or-error in R:
# `if (Position(is.na, c(1, 2, 3)))` raises "argument is not
# interpretable as logical". It can never be the `if (length(x))`
# non-empty idiom, whose suppression requires a stub-declared never-NA
# integer-1 return (ok_if_length_idiom.R). Position's stub deliberately
# omits `na: false`, so the coercion nudge fires like `if (1L)` does.
if (Position(is.na, c(1, 2, 3))) print(1)
