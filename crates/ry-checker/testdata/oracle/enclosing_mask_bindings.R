# oracle: must-pass
x <- function() 1L
stopifnot(local({ x <- 1L; x() }) == 1L)
stopifnot(with(data.frame(x = 1L), x()) == 1L)
y <- list(field = 2L)
local({ y <- 1L })
stopifnot(y$field == 2L)

local <- 1L
x <- function() 1L
local({ x <- 1L })
stopifnot(identical(x(), 1L))
f <- function(local) {
  x <- 1L
  local({ x <- list(field = 1L) })
  x$field
}
stopifnot(identical(f(identity), 1L))
