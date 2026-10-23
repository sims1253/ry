# oracle: must-pass
# `$` on a base data.frame is list `$`: a unique column prefix returns
# that column (#753). A tibble's `$` never partial-matches (it warns and
# returns NULL), so ry keeps RY060 there; see the scope_resolution.R cases.
df <- data.frame(nvalid = 1:3, total = c(2, 4, 6))
stopifnot(identical(df$total / df$nv, c(2, 2, 2)))
