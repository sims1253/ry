# oracle: must-pass
skip_and <- function(x = x) FALSE && x
skip_or <- function(x = x) TRUE || x
dynamic_and <- function(flag, x = x) flag && x
dynamic_or <- function(flag, x = x) flag || x
nested_skip <- function(x = x) TRUE && (TRUE || x)
quoted <- function(x = x) TRUE && base::quote(x)
stopifnot(identical(skip_and(), FALSE), identical(skip_or(), TRUE))
stopifnot(identical(dynamic_and(FALSE), FALSE), identical(dynamic_or(TRUE), TRUE))
stopifnot(identical(nested_skip(), TRUE))
# quote returns a symbol, so && rejects its type without forcing x.
result <- tryCatch(quoted(), error = function(e) e)
stopifnot(inherits(result, "error"), !grepl("promise already under evaluation", conditionMessage(result)))
