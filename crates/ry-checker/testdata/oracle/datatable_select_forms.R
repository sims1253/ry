# oracle: must-pass
# `[.data.table` interprets `-<character>` / `!<character>` in `j` as
# column drops and `!<character>` / `!<list>` in `i` as key exclusion /
# not-join (issue #367). R proves all four forms; ry must stay silent
# because the data.table receiver is opaque to inference. The selector
# role follows the argument tag, not the positional slot: named `j` at
# the first slot selects, and `.SDcols` takes both documented inversion
# spellings at any slot.
library(data.table)
dt <- as.data.table(data.frame(k = c("a", "b", "c"), x = 1:3))
setkey(dt, k)
stopifnot(identical(names(dt[, -c("k")]), "x"))
stopifnot(identical(names(dt[, !"k"]), "x"))
stopifnot(identical(dt[!"a"]$k, c("b", "c")))
stopifnot(identical(dt[!list(k = "a")]$k, c("b", "c")))
stopifnot(identical(names(dt[j = -c("k")]), "x"))
stopifnot(identical(names(dt[, .SD, .SDcols = !c("k")]), "x"))
stopifnot(identical(names(dt[, .SD, .SDcols = -c("k")]), "x"))
