# oracle: must-flag
# #650: the `(A, character)` method answers `g(new("A"), "text")` whatever
# the registration order, so its character result reaches `+ 1L`.
invisible(methods::setClass("A", slots = c(value = "integer")))
invisible(methods::setGeneric("g", function(x, y) standardGeneric("g")))
invisible(methods::setMethod("g", signature(x = "A", y = "character"), function(x, y) "oops"))
invisible(methods::setMethod("g", signature(x = "A", y = "numeric"), function(x, y) 1L))
g(methods::new("A"), "text") + 1L
