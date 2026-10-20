# oracle: must-pass
# A project `::` runs instead of base `length`, and rebinds `cls` in its
# caller. A qualified head counts as base only while `::` itself does.
`::` <- function(pkg, name) function(...) assign("cls", function(x) x, envir = parent.frame())
run <- function() {
  cls <- function(x) { stopifnot(identical(class(x), "integer")); x }
  base::length("a")
  cls("after")
}
stopifnot(identical(run(), "after"))
