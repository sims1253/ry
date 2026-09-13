# oracle: must-pass
# A data-mask column named like a base function (`table`) is data at
# runtime: `subset()` and `with()` resolve it against the receiver's
# columns before the search path, so the comparisons below succeed.
# Resolving `table` to base::table() instead would error ("comparison
# of these types is not implemented"), failing this premise (#369).
dat <- data.frame(table = c("4", "6"), penalty = c(1L, 2L))
s <- subset(dat, table == "6")
stopifnot(nrow(s) == 1L, identical(s$penalty, 2L))
w <- with(dat, table == "6")
stopifnot(is.logical(w), identical(w, c(FALSE, TRUE)))
