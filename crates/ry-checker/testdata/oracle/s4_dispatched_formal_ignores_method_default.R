# oracle: must-pass
# A formal the selected signature names receives the supplied actual, so the
# method's own default says nothing about its type.
invisible(setClass("A", slots = c(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "numeric"), function(x, y = "text") y))
invisible(setMethod("g", signature("A", "character"), function(x, y) 1L))
r <- g(new("A"), 1)
stopifnot(identical(r + 1L, 2))
