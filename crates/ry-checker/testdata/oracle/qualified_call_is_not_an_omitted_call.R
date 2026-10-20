# oracle: must-pass
# `base::date()` does not call the project `date`, so it is no evidence that
# the project function's default is ever selected.
date <- function(offset = "none") offset + 1L
stopifnot(identical(date(1L), 2L))
stopifnot(is.character(base::date()))
