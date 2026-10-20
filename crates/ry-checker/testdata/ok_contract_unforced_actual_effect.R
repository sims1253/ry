# no-diag
# A call contract reads only the actual bound to its formal. The `...`
# actual `{v <- "a"}` is never forced, so `v` is still 1L when bound to `x`.
f <- function(x, ...) x + 1L
id <- function(x, ...) x
v <- 1L
f({v <- "a"}, x = v)
id({v <- "a"}, x = v) + 1L
