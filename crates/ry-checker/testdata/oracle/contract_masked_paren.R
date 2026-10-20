# oracle: must-pass
# `(` is an ordinary binding, so a parenthesized body proves nothing.
id <- function(x) (x)
f <- function(x) (x) + 1L
`(` <- function(x) 10L
stopifnot(identical(id("a") + 1L, 11L), identical(f("a"), 11L))
