# oracle: must-flag
# Base R does not data-mask `[`: `[.data.frame` evaluates `i`/`j` as
# ordinary promises in the calling frame, so `month` below is the
# closure, and comparing a closure with an atomic errors (R: "comparison
# (==) is possible only for atomic and list types"). The eager RY030 is
# a true positive; the data-mask column-first fix for a base data.frame
# is `flights$month == 6L`, `with()`, or `subset()` (#369).
month <- function(x) format(x)
flights <- data.frame(month = 1:12, dep_time = 401:412)
june <- flights[month == 6L]
