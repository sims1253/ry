# no-diag
# `transform(df, ...)` evaluates each named column expression in a scope
# augmented with `df`'s columns. `mpg` resolves to the `mtcars` column
# type, so `mpg * 0.425` is well-typed arithmetic.
df <- mtcars
df2 <- transform(df, kml = mpg * 0.425)
