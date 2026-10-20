# oracle: must-pass
# A global `+` replaces the base operator in the body.
g <- function(x) x + 1
`+` <- function(e1, e2) paste(e1, e2)
stopifnot(identical(g("fine"), "fine 1"))
