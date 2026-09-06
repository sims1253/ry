# no-diag
# The body forces the default, but the default never evaluates its own formal.
f_if_false <- function(x = if (FALSE) x else 1L) x
f_if_true <- function(x = if (TRUE) 1L else x) x
f_and <- function(x = FALSE && x) x
f_or <- function(x = TRUE || x) x
f_block <- function(x = { if (FALSE) x; 1L }) x
f_nested <- function(x = if (TRUE) { FALSE && x } else x) x
