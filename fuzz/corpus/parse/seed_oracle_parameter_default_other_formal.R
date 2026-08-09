# oracle: must-pass
valid_default <- function(a, b = a) b
stopifnot(identical(valid_default(1L), 1L))
