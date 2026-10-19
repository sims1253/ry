# oracle: must-pass
# `t()` on an instance of `A` returns a `B` here, so a value computed from
# an instance is not that instance: `(C, B)` answers, not `(C, A)`.
invisible(setClass("A", contains = "integer"))
invisible(setClass("B", representation(v = "numeric")))
invisible(setClass("C", representation(v = "numeric")))
invisible(setMethod("t", "A", function(x) new("B")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("C", "A"), function(x, y) "text"))
invisible(setMethod("g", signature("C", "B"), function(x, y) 1L))
stopifnot(identical(g(new("C"), t(new("A", 1:3))) + 1L, 2L))
b <- t(new("A", 1:3))
stopifnot(identical(g(new("C"), b) + 1L, 2L))
