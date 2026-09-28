# oracle: must-warn RY032
# An empty iterator preserves x's length-two value; a plain alias after
# the loop must preserve that reachable vector alternative.
f <- function(xs) {
  x <- c(1L, 2L)
  for (i in xs) x <- 1L
  y <- x
  if (y == 1L && TRUE) y
}
stopifnot(identical(do.call(f, list(1L)), 1L))
vector_error <- tryCatch(do.call(f, list(integer())), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
