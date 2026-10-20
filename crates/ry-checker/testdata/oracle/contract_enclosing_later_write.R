# oracle: must-pass
# `g` runs after its enclosing frame rebinds `f` to `paste` (S7).
f <- function(x) x + 1L
h <- function() { g <- function() f("a"); f <- paste; g() }
stopifnot(identical(h(), "a"))
