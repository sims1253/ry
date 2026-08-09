# no-diag
setClass("SpatExtent", representation(ptr = "numeric"))
setMethod("as.vector", signature(x = "SpatExtent"), function(x, mode = "any") {
  c(xmin = 1, xmax = 2, ymin = 3, ymax = 4)
})

e <- new("SpatExtent")
v <- as.vector(e)
v[["xmin"]]
v["xmin"]
