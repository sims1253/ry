`&&` <- function(x, y) TRUE
f <- function(xs) { x <- c(1L, 2L); for (i in xs) { if (x == 1L && TRUE) i; x <- 1L } }
f(1L)
