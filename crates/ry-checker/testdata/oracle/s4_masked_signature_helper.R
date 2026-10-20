# oracle: must-pass
# A project `signature` is not `methods::signature`: this registration is
# `(A, character)`, so `g(new("A"), 2)` reaches `(A, ANY)`.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
signature <- function(...) methods::signature("A", "character")
invisible(setMethod("g", signature("A", "numeric"), function(x, y) "text"))
invisible(setMethod("g", methods::signature("A", "ANY"), function(x, y) 1L))
stopifnot(identical(g(methods::new("A"), 2) + 1L, 2L))
