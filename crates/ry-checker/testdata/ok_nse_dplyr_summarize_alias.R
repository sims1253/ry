# no-diag
# dplyr's American-English alias `summarize` is the same verb as
# `summarise`. The column reference `hp` resolves against the augmented
# scope built from `mtcars`, so the call is well-typed.
library(dplyr)
df <- mtcars
s <- summarize(df, m = mean(hp))
