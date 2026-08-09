# no-diag
# `filter(df, condition)` is dplyr's row-filtering verb. The first arg
# is the data frame; the condition is evaluated in a scope augmented
# with `df`'s columns, so `mpg` resolves to the `mtcars` column type
# and the comparison is well-typed.
library(dplyr)
df <- mtcars
small <- filter(df, mpg > 20)
