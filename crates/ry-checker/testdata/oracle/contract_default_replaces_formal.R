# oracle: must-pass
# Forcing `y` replaces `x` before the arithmetic (S9).
f <- function(x, y = {x <- 1L; NULL}) {y; x + 1L}
stopifnot(identical(f("fine"), 2L))
