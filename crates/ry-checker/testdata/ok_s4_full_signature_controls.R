# no-diag
# Proven S4 selections whose results are numeric.
setClass("A", representation(v = "numeric"))
setMethod("as.vector", signature(x = "A"), function(x, mode = "any") c(lo = 1, hi = 2))
as.vector(new("A"))[["lo"]] + 1
setGeneric("k", function(x, y) standardGeneric("k"))
setMethod("k", signature("A", "missing"), function(x, y) 1L)
setMethod("k", signature("A", "character"), function(x, y) "chr")
setMethod("k", signature("A", "logical"), function(x, y) 2L)
k(new("A")) + 1L
k(new("A"), TRUE) + 1L
