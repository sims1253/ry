# oracle: must-flag
# Base R has no negative character subscript: the unary `-` errors even
# though it sits inside `[` (issue #367 control for RY020).
v <- c("x", "y")
u <- v[-c("x")]
