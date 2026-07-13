# oracle: must-warn RY090
# R rejects an unmatched named argument when the function has no `...`.
error <- tryCatch(length(xx = 1L), error = function(condition) condition)
stopifnot(inherits(error, "error"))
