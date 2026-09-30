# oracle: must-pass
`<` <- function(lhs, rhs) TRUE
f <- function(xs) { x <- c(1L, 2L); for (i in xs) x <- 1L; y <- x; if (0L < y && TRUE) y }; stopifnot(identical(do.call(f, list(integer())), c(1L, 2L)))
