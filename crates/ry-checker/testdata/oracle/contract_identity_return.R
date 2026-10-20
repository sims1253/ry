# oracle: must-flag-only RY040
# A bare-formal body returns its actual, so the error is in the caller (S10).
id <- function(x) x
id("a") + 1
