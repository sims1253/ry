# oracle: must-pass
# A `dim` attribute makes a double dispatch as `matrix`, not `numeric`.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "numeric"), function(x, y) "num"))
invisible(setMethod("g", signature("A", "matrix"), function(x, y) 1L))
stopifnot(identical(g(new("A"), structure(1, dim = c(1L, 1L))) + 1L, 2L))
