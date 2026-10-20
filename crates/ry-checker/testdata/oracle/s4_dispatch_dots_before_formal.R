# oracle: must-pass
# A positional actual after `x` goes to `...`, so `y` is missing.
invisible(setClass("A", representation(v = "numeric")))
invisible(setClass("B", representation(v = "numeric")))
invisible(setGeneric("k", function(x, ..., y) standardGeneric("k")))
invisible(setMethod("k", signature("A", "B"), function(x, ..., y) "ab"))
invisible(setMethod("k", signature("A", "missing"), function(x, ..., y) 1L))
stopifnot(identical(k(new("A"), new("B")) + 1L, 2L))
