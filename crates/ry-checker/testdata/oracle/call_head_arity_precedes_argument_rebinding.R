# oracle: must-warn RY091
f <- function(x, y) { x; y; 1L }
failure <- tryCatch(f({ f <- function(x) { x; 1L }; 1L }), error = identity)
stopifnot(inherits(failure, "error"), grepl('"y"', conditionMessage(failure)))
