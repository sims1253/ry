# oracle: must-pass
# `{` is an ordinary binding, so `{ x }` is not an identity.
id <- function(x) { x }
`{` <- function(...) 10L
stopifnot(identical(id("a") + 1L, 11L))
