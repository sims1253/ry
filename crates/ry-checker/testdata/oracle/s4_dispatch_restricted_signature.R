# oracle: must-pass
# `signature = "x"` dispatches on the second formal, not the first actual.
invisible(setClass("A", representation(v = "numeric")))
invisible(setClass("B", representation(v = "numeric")))
invisible(setGeneric("g", function(z, x) standardGeneric("g"), signature = "x"))
invisible(setMethod("g", "A", function(z, x) "a"))
invisible(setMethod("g", "B", function(z, x) 1L))
stopifnot(identical(g(new("A"), new("B")), 1L))
stopifnot(identical(g(new("A"), new("B")) + 1L, 2L))
