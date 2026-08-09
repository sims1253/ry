# no-diag
# `if (length(x))` / `if (nrow(df))` / `if (ncol(df))` are the idiomatic
# non-empty checks in real R code. `length`/`nrow`/`ncol` return an integer
# length-1 that R silently coerces to logical, but warning about that
# coercion here is pure noise. RY003's numeric-truthiness arm is suppressed for a
# direct call to `length`/`nrow`/`ncol` (bare identifier callee, any args).
x <- c(1, 2, 3)
if (length(x)) print(1)
d <- data.frame(a = 1)
if (nrow(d)) print(2)
if (ncol(d)) print(3)
