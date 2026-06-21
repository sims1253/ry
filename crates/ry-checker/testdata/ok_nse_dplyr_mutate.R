# no-diag
# `mutate(df, kml = mpg * 0.425)` adds a new column whose value depends
# on an existing column. The expression is evaluated against an
# augmented scope where `mpg` resolves to the `mtcars` column type, so
# the arithmetic is well-typed.
df <- mtcars
df2 <- mutate(df, kml = mpg * 0.425)
