# oracle: must-pass
# An integer inherits from `double`, which is nearer than `ANY`.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "ANY"), function(x, y) "any"))
invisible(setMethod("g", signature("A", "double"), function(x, y) 1L))
stopifnot(identical(g(new("A"), 2L) + 1L, 2L))
