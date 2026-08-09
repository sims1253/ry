# no-diag
# `with(df, expr)` evaluates `expr` in a scope augmented with `df`'s
# columns. `mpg` resolves to the `mtcars` column type, and `sum(mpg)`
# dispatches against the typeshed to a length-1 numeric.
df <- mtcars
total <- with(df, sum(mpg))
