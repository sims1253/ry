# oracle: must-pass
# oracle-claim: RY105
# sum() constructs one value, so its length is one even for an empty input.
stopifnot(identical(length(sum(numeric())), 1L))
stopifnot(isTRUE(length(sum(1:3)) > 0L))
