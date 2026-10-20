# oracle: must-flag
# An unqualified omitted call does select the project `date`'s default.
date <- function(offset = "none") offset + 1L
stopifnot(identical(date(1L), 2L))
date()
