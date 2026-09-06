# no-diag
skip_and <- function(x = x) FALSE && x
skip_or <- function(x = x) TRUE || x
dynamic_and <- function(flag, x = x) flag && x
dynamic_or <- function(flag, x = x) flag || x
nested_skip <- function(x = x) TRUE && (TRUE || x)
quoted <- function(x = x) TRUE && base::quote(x)
