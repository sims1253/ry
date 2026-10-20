# oracle: known-gap a deferred call may run before a later same-file definition (S5); project arity (RY090/RY091) shares the gap
# `g()` runs while `sort` is still base `sort`.
g <- function() sort("a")
stopifnot(identical(g(), "a"))
sort <- function(x) x + 1L
