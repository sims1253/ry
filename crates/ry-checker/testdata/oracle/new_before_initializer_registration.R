# oracle: must-flag
methods::setClass("ry_before_initializer", slots = c(value = "integer"))
methods::new("ry_before_initializer", "ignored" + 1L)
methods::setMethod("initialize", "ry_before_initializer", function(.Object, ...) .Object)
