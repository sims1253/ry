# oracle: must-flag
methods::setClass("ry_removed_initializer", slots = c(value = "integer"))
methods::setMethod("initialize", "ry_removed_initializer", function(.Object, ...) .Object)
methods::removeMethod("initialize", "ry_removed_initializer")
methods::new("ry_removed_initializer", "ignored" + 1L)
