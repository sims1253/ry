# oracle: must-pass
# `$` on a base data.frame or a data.table is list `$`: a unique column
# prefix returns that column (#753). A tibble's `$` never partial-matches
# (it warns and returns NULL), so ry keeps RY060 there; see the
# diagnostics.R cases.
library(data.table)
df <- data.frame(nvalid = 1:3, total = c(2, 4, 6))
stopifnot(identical(df$total / df$nv, c(2, 2, 2)))
stopifnot(identical(as.data.table(df)$nv, 1:3))
