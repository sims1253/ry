# oracle: must-pass
# Forcing `z` inside `y`'s default binds `f` to `paste` in `h`'s frame (S8).
f <- function(x) x + 1L
h <- function(y = { z; f("a") }, z = { f <- paste; NULL }) y
stopifnot(identical(h(), "a"))
