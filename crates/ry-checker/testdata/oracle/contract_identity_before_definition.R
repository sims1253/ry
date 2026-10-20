# oracle: must-pass
# An eager call before the definition reaches base `length`, not the identity.
y <- length("ab")
length <- function(x) x
stopifnot(identical(y + 1L, 2L))
