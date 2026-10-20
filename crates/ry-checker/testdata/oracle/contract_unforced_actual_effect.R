# oracle: must-pass
# Only the actual bound to `x` is forced; `{v <- "a"}` lands in `...` and never runs.
f <- function(x, ...) x + 1L
id <- function(x, ...) x
v <- 1L
stopifnot(identical(f({v <- "a"}, x = v), 2L))
stopifnot(identical(id({v <- "a"}, x = v) + 1L, 2L))
