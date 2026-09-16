# oracle: must-pass
# oracle-claim: RY108
# The premise of RY108 in three R-verified facts:
#
# 1. A defaulted formal is still `missing()` when the caller omits it, and
#    forcing the promise does not change the answer; only rebinding the
#    name collapses `missing()` to FALSE. Supply therefore has two states
#    R keeps apart (`missing()`), which is what the rule asks the method
#    to consult before using `to`.
# 2. `seq.default` applies argument precedence: a forwarded `to` loses to
#    `length.out`, so `seq(1, 1, length.out = 3)` repeats the endpoint
#    three times instead of building 1:3.
# 3. Forwarding a defaulted `to` alongside `by` errors ("too many
#    arguments"), the loud form of the same collision.
stopifnot(isTRUE((function(to = 5) missing(to))()))
stopifnot(isFALSE((function(to = 5) missing(to))(1)))
stopifnot(isTRUE((function(to = 5) {
  forced <- to + 1
  missing(to)
})()))
stopifnot(isFALSE((function(to = 5) {
  to <- to + 1
  missing(to)
})()))
stopifnot(identical(seq(1, 1, length.out = 3), c(1, 1, 1)))
stopifnot(identical(
  tryCatch(seq(1, 1, by = 2, length.out = 3), error = function(e) "error"),
  "error"
))
