# oracle: must-flag
methods::setClass("ry_masked_initializer", slots = c(value = "integer"))
setMethod <- function(...) invisible(NULL)
setMethod("initialize", "ry_masked_initializer", function(.Object, ...) .Object)
methods::new("ry_masked_initializer", "ignored" + 1L)
