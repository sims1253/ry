# oracle: must-pass
# A generic whose body does more than forward returns its own value.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) {
  standardGeneric("g")
  1L
}))
invisible(setMethod("g", signature("A", "character"), function(x, y) "method"))
stopifnot(identical(g(new("A"), "t") + 1L, 2L))
