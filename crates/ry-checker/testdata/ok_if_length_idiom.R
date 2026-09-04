# no-diag
# `if (length(x))` / `if (nrow(df))` / `if (ncol(df))` are the idiomatic
# non-empty checks in real R code. Their stubs return an integer length-1
# that R silently coerces to logical, but warning about that coercion
# here is pure noise. RY003's numeric-truthiness arm is suppressed for a
# direct call whose resolved stub declares an integer length-1 return —
# the original three plus everything the stubs record the same way
# (`NROW`, `NCOL`, `nobs`, `Position`, vctrs' `vec_size`, ...).
x <- c(1, 2, 3)
if (length(x)) print(1)
d <- data.frame(a = 1)
if (nrow(d)) print(2)
if (ncol(d)) print(3)
if (NROW(d)) print(4)
if (NCOL(d)) print(5)
if (base::length(x)) print(6)
fit <- NULL
if (nobs(fit)) print(7)
if (Position(is.na, x)) print(8)
library(vctrs)
if (vec_size(x)) print(9)
