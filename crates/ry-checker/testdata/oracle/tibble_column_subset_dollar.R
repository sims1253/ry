# oracle: must-flag
# A tibble's `[` keeps the tibble class and drops only on `drop = TRUE`,
# so `$` on a column subset never partial-matches: it warns "Unknown or
# uninitialised column" and returns NULL (#771). `warn = 2` turns that
# warning into the error ry's RY060 predicts. A base data.frame subset
# does partial-match.
options(warn = 2)
df <- data.frame(nvalid = 1:3, k = 1:3)
stopifnot(identical(df[, "nvalid", drop = FALSE]$nv, 1:3))
stopifnot(identical(df[, 1, drop = FALSE]$nv, 1:3))
tb <- dplyr::group_by(df)
stopifnot(identical(class(tb[, "nvalid"]), c("tbl_df", "tbl", "data.frame")))
# The third positional argument of `[` is `drop`.
stopifnot(is.integer(tb[, "nvalid", TRUE]), inherits(tb[, "nvalid", FALSE], "tbl_df"))
tb[, "nvalid"]$nv
