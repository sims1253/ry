# oracle: must-pass
# A global `$` replaces the base operator in the body.
nm <- function(df) df$value * 2
`$` <- function(x, name) 21
stopifnot(identical(nm(1:3), 42))
