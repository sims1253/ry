# oracle: must-warn RY107
# oracle-claim: RY107
# any()/all() construct a length-1 logical; comparing that scalar with a
# numeric literal coerces TRUE/FALSE to 1/0, so `== 0` computes the
# negation, `> 1` is constant FALSE, and a negative bound like `> -1` is
# constant TRUE -- 0 and 1 both clear it, for an all-FALSE vector and even
# for the vacuous empty one. None is the element-level test the author of
# glue R/utils.R:32 intended: with a zero length present the written guard
# is FALSE while `any(lengths == 0)` is TRUE.
lens <- c(0L, 3L)
stopifnot(identical(length(any(lens)), 1L), is.logical(all(lens)))
stopifnot(identical(any(lens) == 0, !any(lens)))
stopifnot(identical(any(lens) == 0, FALSE))
stopifnot(identical(any(lens == 0), TRUE))
stopifnot(identical(any(lens) > 1, FALSE))
stopifnot(identical(any(c(FALSE, FALSE)) > -1, TRUE))
stopifnot(identical(any(logical()) > -1, TRUE))
stopifnot(identical(any(lens) < -1, FALSE))
negated <- any(lens) == 0
negated
beyond_minus_one <- any(lens) > -1
beyond_minus_one
