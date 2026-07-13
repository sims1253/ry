# oracle: must-warn RY091
# A directly forced missing formal errors when the function body evaluates it.
required <- function(x) x
error <- tryCatch(required(), error = function(condition) condition)
stopifnot(inherits(error, "error"))
