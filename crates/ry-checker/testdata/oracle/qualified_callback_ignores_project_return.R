# oracle: must-pass
# `base::identity` passed as a callback is base's function, not the project's.
identity <- function(x) "project"
r <- sapply(1:3, base::identity)
stopifnot(identical(r, 1:3))
r + 1L
