# no-diag
# `summarise(df, m = mean(mpg))` collapses the data frame to a single
# row. The aggregation expression `mean(mpg)` is evaluated against an
# augmented scope where `mpg` resolves, so the call is well-typed. The
# result is a new data frame type (the input columns no longer apply).
library(dplyr)
df <- mtcars
summary <- summarise(df, m = mean(mpg), n = n())
