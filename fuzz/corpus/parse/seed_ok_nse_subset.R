# no-diag
# `subset(df, subset_expr)` evaluates `subset_expr` in a scope augmented
# with `df`'s columns. `cyl` and `mpg` resolve to the `mtcars` column
# types, so the boolean expression is well-typed.
df <- mtcars
small <- subset(df, cyl == 4 & mpg > 25)
