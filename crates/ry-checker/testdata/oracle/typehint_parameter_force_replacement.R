# oracle: must-pass
# Reading a formal forces its promise, and the caller's promise code can
# replace a name the callee resolves outward: `cls("after")` reaches the
# replacement. ry drops the identity after `length(y)`, but still keeps it
# across a direct read of `y` (known gap, tests/declarations.rs).
cls <- function(x) { stopifnot(identical(class(x), "integer")); x }
stopifnot(inherits(tryCatch(cls("before"), error = identity), "error"))
direct <- function(y) {
  y
  cls("after")
}
stopifnot(identical(direct(y = {assign("cls", function(x) x); 1L}), "after"))
cls <- function(x) { stopifnot(identical(class(x), "integer")); x }
stopifnot(inherits(tryCatch(cls("before"), error = identity), "error"))
counted <- function(y) {
  length(y)
  cls("after")
}
stopifnot(identical(counted(y = {assign("cls", function(x) x); 1L}), "after"))
