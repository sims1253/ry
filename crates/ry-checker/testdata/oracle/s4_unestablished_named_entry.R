# oracle: must-pass
# An aliased `setGeneric` leaves the formals unknown: `y = "A"` cannot be
# placed, and the generic's own body answers this call.
invisible(setClass("A", representation(v = "numeric")))
sg <- methods::setGeneric
invisible(sg("g", function(x, y) 1L))
invisible(setMethod("g", signature(y = "A"), function(x, y) "method"))
stopifnot(identical(g(new("A"), 2) + 1L, 2L))
