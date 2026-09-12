# oracle: must-pass
x <- function() 1L
stopifnot(local({ x <- 1L; x() }) == 1L)
stopifnot(with(data.frame(x = 1L), x()) == 1L)
y <- list(field = 2L)
local({ y <- 1L })
stopifnot(y$field == 2L)
