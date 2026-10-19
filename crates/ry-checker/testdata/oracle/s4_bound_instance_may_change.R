# oracle: must-pass
# A binding of `new("A")` can be replaced or modified before it is read,
# so a variable is never exact dispatch evidence.
invisible(setClass("A", representation(v = "numeric")))
invisible(setClass("B", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "character"), function(x, y) "text"))
invisible(setMethod("g", signature("B", "character"), function(x, y) 1L))
`foo<-` <- function(x, value) methods::new("B")
a <- methods::new("A")
foo(a) <- 1
stopifnot(identical(g(a, "t") + 1L, 2L))
d <- methods::new("A")
assign("d", methods::new("B"), envir = globalenv())
stopifnot(identical(g(d, "t") + 1L, 2L))
