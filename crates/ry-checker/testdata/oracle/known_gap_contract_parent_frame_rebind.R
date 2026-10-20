# oracle: known-gap `parent.frame()`/`environment()` handles are not treated as binding installers (too common to give up call contracts for)
# `k()` rebinds the global `f` through its caller's frame object.
f <- function(x) x + 1L
k <- function() { e <- parent.frame(); e$f <- paste }
k()
stopifnot(identical(f("a"), "a"))
