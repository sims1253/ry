# oracle: must-pass
# `inherits(x, "A")` admits the subclass `B`, whose own method answers.
invisible(setClass("A", representation(v = "numeric")))
invisible(setClass("B", contains = "A"))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "character"), function(x, y) "a"))
invisible(setMethod("g", signature("B", "character"), function(x, y) 1L))
f <- function(x) {
  stopifnot(inherits(x, "A"))
  g(x, "t") + 1L
}
stopifnot(identical(f(new("B")), 2L))
