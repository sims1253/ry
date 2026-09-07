# oracle: must-flag
# A literal method body is not the body that setMethod must install.
old <- methods::selectMethod("initialize", "MethodDefinition")
methods::setMethod("initialize", "MethodDefinition", function(.Object, ...) {
  result <- old(.Object, ...)
  result@.Data <- function(.Object, ...) { list(...); .Object }
  result
})
methods::setClass("ry_metadata_replacement", slots = c(value = "integer"))
methods::setMethod("initialize", "ry_metadata_replacement", function(.Object, ...) .Object)
stopifnot(identical(
  body(methods::selectMethod("initialize", "ry_metadata_replacement")),
  quote({ list(...); .Object })
))
methods::new("ry_metadata_replacement", "ignored" + 1L)
