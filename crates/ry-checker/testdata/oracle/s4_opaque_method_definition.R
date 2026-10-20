# oracle: must-pass
# The later registration of the same signature, bound to `helper`, answers.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "character"), function(x, y) "chr"))
helper <- function(x, y) 1L
invisible(setMethod("g", signature("A", "character"), helper))
stopifnot(identical(g(new("A"), "t") + 1L, 2L))
