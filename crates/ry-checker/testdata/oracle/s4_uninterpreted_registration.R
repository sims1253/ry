# oracle: must-pass
# A `c()` signature registers a method too: the recorded ones are not all.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "character"), function(x, y) "chr"))
invisible(setMethod("g", c("A", "character"), function(x, y) 1L))
stopifnot(identical(g(new("A"), "t") + 1L, 2L))
invisible(setGeneric("h", function(x, y) standardGeneric("h")))
invisible(setMethod("h", "A", function(x, y) "chr"))
invisible(setMethod("h", c(x = "A", y = "character"), function(x, y) 1L))
stopifnot(identical(h(new("A"), "t") + 1L, 2L))
