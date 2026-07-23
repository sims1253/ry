# oracle: must-pass
# A parameter may receive a scalar at every call site. Unknown length alone is
# not evidence that &&/|| discards vector elements.
f <- function(x, y) x && y
stopifnot(isTRUE(f(TRUE, TRUE)))
