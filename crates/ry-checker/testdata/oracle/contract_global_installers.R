# oracle: must-pass
# `eval.parent()` and `load()` can replace the global `f` and `id` the calls reach.
f <- function(x) x + 1L
id <- function(x) x
k <- function() eval.parent(quote(f <- paste))
k()
stopifnot(identical(f("a"), "a"))
path <- tempfile()
id <- function(x) 1L
save(id, file = path)
id <- function(x) x
load(path)
stopifnot(identical(id("a") + 1L, 2L))
