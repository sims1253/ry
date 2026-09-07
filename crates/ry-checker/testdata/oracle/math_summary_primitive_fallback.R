# oracle: must-pass
# Unrelated group methods do not prevent built-in member fallback.
Summary.other <- function(..., na.rm = FALSE) 99L
Math.other <- function(x, ...) 99
x <- structure(c(1L, 2L), class = "widget")
stopifnot(sum(x) == 3L, prod(x) == 2, max(x) == 2L, min(x) == 1L)
stopifnot(all(x), any(x), identical(as.numeric(range(x)), c(1, 2)))
stopifnot(identical(as.numeric(abs(x)), c(1, 2)))
stopifnot(identical(as.numeric(sqrt(x)), sqrt(c(1, 2))))
tab <- structure(c(2L, 1L), dim = 2L, class = "table")
stopifnot(sum(tab) == 3L)
