# oracle: must-pass
# `load()` binds into its caller's frame by default, so `f` is `paste` there.
f <- function(x) x + 1L
h <- function(path) { load(path); f("a") }
path <- tempfile()
saved <- new.env()
saved$f <- paste
save(list = "f", envir = saved, file = path)
stopifnot(identical(h(path), "a"))
