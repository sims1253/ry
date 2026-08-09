# oracle: must-pass
# oracle-claim: RY100
# The comparison is evaluated before abs(), which converts FALSE/TRUE to 0/1.
stopifnot(identical(abs(1L > 2L), 0L))
stopifnot(identical(abs(2L > 1L), 1L))
