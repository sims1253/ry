# oracle: must-pass
# Environment-targeting binders install a local `f`/`id` that the calls reach.
f <- function(x) x + 1L
id <- function(x) x
h1 <- function() { list2env(list(f = paste), envir = environment()); f("a") }
h2 <- function() { list2env(list(id = function(x) 1L), envir = environment()); id("a") + 1L }
h3 <- function() { source(textConnection("f <- paste"), local = TRUE); f("a") }
stopifnot(identical(h1(), "a"), identical(h2(), 2L), identical(h3(), "a"))
