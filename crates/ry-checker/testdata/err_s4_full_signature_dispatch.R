# expect: RY040
# #650: S4 selection uses the full signature, so `(A, character)` answers
# regardless of registration order. R: non-numeric argument to binary operator.
methods::setClass("A", slots = c(value = "integer"))
methods::setGeneric("g", function(x, y) standardGeneric("g"))
methods::setMethod("g", signature(x = "A", y = "character"), function(x, y) "oops")
methods::setMethod("g", signature(x = "A", y = "numeric"), function(x, y) 1L)
g(methods::new("A"), "text") + 1L
