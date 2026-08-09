# no-diag
# `arrange(df, col)` sorts rows by `col`; `group_by(df, col)` groups by
# `col`. Both evaluate their column references against an augmented
# scope, so `mpg` resolves and the calls are well-typed.
library(dplyr)
df <- mtcars
sorted <- arrange(df, mpg)
grouped <- group_by(df, cyl)
