# oracle: must-warn RY107
# oracle-claim: RY107
# any()/all() construct a length-1 logical; comparing that scalar with a
# numeric literal coerces TRUE/FALSE to 1/0, so `== 0` computes the
# negation, `> 1` is constant FALSE, and a negative bound like `> -1` is
# constant TRUE -- 0 and 1 both clear it, for an all-FALSE vector and even
# for the vacuous empty one. The constant outcome is scoped to the base
# domain's non-NA results: an undetermined NA (no TRUE present for
# any(), no FALSE for all()) propagates, and NA compares as NA rather
# than as 0 or 1, so the diagnostic words constant claims as "when the
# base result is not NA". Method dispatch can leave the domain entirely,
# and that fact bounds the rule's applicability, not just its wording:
# the Summary-group witness below makes any(sgrp) return 42, so
# any(sgrp) == 42 is a meaningful comparison under the method (the
# default-domain table would call it constant FALSE), and the checker
# stays silent once the argument's class is established -- while the
# open-world lines above (plain vectors) keep their dispatch-qualified
# warning. None is the element-level test the author of glue
# R/utils.R:32 intended: with a zero length present the written guard is
# FALSE while `any(lengths == 0)` is TRUE.
lens <- c(0L, 3L)
stopifnot(identical(length(any(lens)), 1L), is.logical(all(lens)))
stopifnot(identical(any(lens) == 0, !any(lens)))
stopifnot(identical(any(lens) == 0, FALSE))
stopifnot(identical(any(lens == 0), TRUE))
stopifnot(identical(any(lens) > 1, FALSE))
stopifnot(identical(any(c(FALSE, FALSE)) > -1, TRUE))
stopifnot(identical(any(logical()) > -1, TRUE))
stopifnot(identical(any(lens) < -1, FALSE))
stopifnot(identical(any(c(FALSE, NA)), NA))
stopifnot(identical(any(c(TRUE, NA)), TRUE))
stopifnot(identical(all(c(TRUE, NA)), NA))
stopifnot(identical(all(c(FALSE, NA)), FALSE))
stopifnot(is.na(any(c(FALSE, NA)) > -1))
stopifnot(is.na(any(c(FALSE, NA)) == 0))
stopifnot(is.na(all(c(TRUE, NA)) > -1))
stopifnot(identical(any(c(TRUE, NA)) > -1, TRUE))
# A dispatched method replaces the base result: the Summary group
# intercepts any() for S3 classes too, not only for S4 setMethod, so
# this classed argument is the rule's quiet boundary -- the comparison
# with 42 below is meaningful under the method, and ry must not apply
# the FALSE/TRUE outcome heuristic to it.
Summary.s3grp <- function(x, ...) 42
sgrp <- structure(c(FALSE, TRUE), class = "s3grp")
stopifnot(identical(any(sgrp), 42))
stopifnot(identical(any(sgrp) == 42, TRUE))
negated <- any(lens) == 0
negated
beyond_minus_one <- any(lens) > -1
beyond_minus_one
