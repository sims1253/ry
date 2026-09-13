# oracle: must-flag
# Base R has no negated character subscript: the `!` errors even though
# it sits inside `[` (issue #367 control for RY021).
v <- c("x", "y")
u <- v[!"x"]
