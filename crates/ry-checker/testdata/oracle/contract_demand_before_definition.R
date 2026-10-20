# oracle: must-pass
# An eager call before the definition reaches base `sort`, not the demand.
y <- sort("b")
sort <- function(x) x + 1L
stopifnot(identical(y, "b"))
