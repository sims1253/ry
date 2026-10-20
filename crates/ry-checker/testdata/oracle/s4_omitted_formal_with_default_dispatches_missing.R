# oracle: must-pass
# An omitted formal dispatches as `missing` even when the generic gives it a
# default, so `(A, missing)` answers `g(new("A"))`.
invisible(setClass("A", representation(v = "numeric")))
invisible(setGeneric("g", function(x, y = "d") standardGeneric("g")))
invisible(setMethod("g", signature("A", "missing"), function(x, y) 1))
invisible(setMethod("g", signature("A", "character"), function(x, y) "text"))
stopifnot(g(new("A")) + 1L == 2L)
