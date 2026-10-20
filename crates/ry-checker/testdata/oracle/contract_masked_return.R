# oracle: must-pass
# `return` is an ordinary binding, so `return(x)` is not an identity (S14).
`return` <- function(x) 10L
id <- function(x) return(x)
stopifnot(identical(id("a") + 1L, 11L))
