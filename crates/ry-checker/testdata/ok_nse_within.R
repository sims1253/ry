# no-diag
# `within(df, expr)` evaluates `expr` in a scope augmented with `df`'s
# columns. The parser lowers the braced body to its trailing expression
# (`mpg / cyl`), which resolves both column references against the
# `mtcars` schema.
df <- mtcars
df2 <- within(df, { ratio <- mpg / cyl })
