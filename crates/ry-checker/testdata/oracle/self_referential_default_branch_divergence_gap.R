# oracle: known-gap RY109 defusing credit ignores a sibling bare read on another branch: R errors on the else path, ry stays silent
# The defusing branch captures the promise, so body_defuses_formal credits
# it and ry does not warn; on the else path R forces the promise and
# errors. Refusing the credit on any sibling bare read does not fall out
# cleanly: reads after `x <- enquo(x)` see the replacement quosure, not the
# promise (corrr stretch_unique), and formula references are quoted, not
# bare (corrr retract).
suppressMessages(library(rlang))
f <- function(x = x, flag) if (flag) rlang::enquo(x) else x
stopifnot(inherits(f(flag = TRUE), "quosure"))
f(flag = FALSE)
