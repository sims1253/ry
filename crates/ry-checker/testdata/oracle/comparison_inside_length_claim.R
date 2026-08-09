# oracle: must-pass
# oracle-claim: RY093
# A scalar comparison is evaluated before length(), making the result length 1.
stopifnot(identical(length(1L > 2L), 1L))
