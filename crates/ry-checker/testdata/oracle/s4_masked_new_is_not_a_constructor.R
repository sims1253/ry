# oracle: must-pass
# A masked `new` is not `methods::new`: here it is `t`, which turns an `A`
# into a `B`, so neither its result nor a binding of it is an exact `A`.
invisible(setClass("A", contains = "integer"))
invisible(setClass("B", representation(v = "numeric")))
invisible(setClass("C", representation(v = "numeric")))
invisible(setMethod("t", "A", function(x) methods::new("B")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("C", "A"), function(x, y) "text"))
invisible(setMethod("g", signature("C", "B"), function(x, y) 1L))
a <- methods::new("A", 1:3)
new <- t
stopifnot(identical(g(methods::new("C"), new(a)) + 1L, 2L))
b <- new(a)
stopifnot(identical(g(methods::new("C"), b) + 1L, 2L))
