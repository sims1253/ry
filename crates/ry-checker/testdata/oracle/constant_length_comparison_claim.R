# oracle: must-pass
# oracle-claim: RY105
# sum() constructs one value, so its length is one even for an empty input,
# and a length is never negative, so a bound of -1 is as dead as 0.
stopifnot(identical(length(sum(numeric())), 1L))
stopifnot(isTRUE(length(sum(1:3)) > 0L))
stopifnot(isTRUE(length(sum(numeric())) > -1))
stopifnot(identical(length(sum(numeric())) < -1, FALSE))
