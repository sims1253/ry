# oracle: must-pass
quoted_default <- function(x = quote(x)) x
substituted_default <- function(x = substitute(x)) x
stopifnot(identical(quoted_default(), quote(x)))
stopifnot(identical(substituted_default(), quote(substitute(x))))
