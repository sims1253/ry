# oracle: known-gap new() traverses dots that a custom initialize method may leave unforced
methods::setClass("ry_lazy_initialize", slots = c(value = "integer"))
methods::setMethod("initialize", "ry_lazy_initialize", function(.Object, ...) .Object)
stopifnot(inherits(methods::new("ry_lazy_initialize", missing_value), "ry_lazy_initialize"))
