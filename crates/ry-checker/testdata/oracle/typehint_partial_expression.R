# oracle: must-pass
# oracle-claim: RY115
# A simple class assertion does not establish a dimension/value assertion.
x <- matrix(1L, nrow = 1L)
stopifnot(identical(typeof(x), "integer"))
stopifnot(identical(dim(x), c(1L, 1L)))
stopifnot(!identical(dim(x), c(2L, 1L)))
