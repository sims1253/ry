# oracle: must-pass
# An ordered factor's class is `ordered` first: `(A, ordered)` answers, so a
# constructor class is no exact dispatch evidence.
invisible(setClass("A", slots = c(v = "numeric")))
invisible(setGeneric("g", function(x, y) standardGeneric("g")))
invisible(setMethod("g", signature("A", "factor"), function(x, y) "text"))
invisible(setMethod("g", signature("A", "ordered"), function(x, y) 1L))
r <- g(new("A"), factor("a", ordered = TRUE))
stopifnot(identical(r + 1L, 2L))
