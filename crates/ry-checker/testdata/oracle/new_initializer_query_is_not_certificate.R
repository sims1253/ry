# oracle: must-flag
# An active table binding can change registration while returning an old method.
methods::setClass("ry_query_probe", slots = c(value = "integer"))
methods::setMethod("initialize", "ry_query_probe", function(.Object, ...) {
  list(...)
  .Object
})
forcing <- methods::getMethod("initialize", "ry_query_probe")
methods::setMethod("initialize", "ry_query_probe", function(.Object, ...) .Object)
lazy <- methods::getMethod("initialize", "ry_query_probe")
tab <- methods:::getMethodsForDispatch(methods::getGeneric("initialize"))
rm(list = "ry_query_probe", envir = tab)
makeActiveBinding("ry_query_probe", function(value) {
  rm(list = "ry_query_probe", envir = tab)
  methods::setMethod("initialize", "ry_query_probe", forcing)
  lazy
}, tab)
stopifnot(identical(
  body(methods::getMethod("initialize", "ry_query_probe")),
  quote(.Object)
))
methods::new("ry_query_probe", "ignored" + 1L)
