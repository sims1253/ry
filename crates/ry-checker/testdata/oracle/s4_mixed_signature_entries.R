# oracle: must-pass
# Named entries are placed first: `signature(y = "character", "A")` is
# `(x = "A", y = "character")`.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature(y = "character", "A"), function(x, y) 1L))
invisible(setMethod("g", signature("character", "A"), function(x, y) "swapped"))
stopifnot(identical(g(new("A"), "t") + 1L, 2L))
