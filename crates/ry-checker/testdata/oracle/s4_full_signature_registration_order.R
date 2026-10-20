# oracle: must-pass
# The later `(A, character)` registration does not replace `(A, numeric)`.
invisible(methods::setClass("A", slots = c(value = "integer")))
invisible(methods::setGeneric("g", function(x, y) standardGeneric("g")))
invisible(methods::setMethod("g", signature(x = "A", y = "numeric"), function(x, y) 1L))
invisible(methods::setMethod("g", signature(x = "A", y = "character"), function(x, y) "oops"))
stopifnot(identical(g(methods::new("A"), 2), 1L))
stopifnot(identical(g(methods::new("A"), 2) + 1L, 2L))
