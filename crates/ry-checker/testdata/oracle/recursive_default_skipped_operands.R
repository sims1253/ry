# oracle: must-pass
f_if_false <- function(x = if (FALSE) x else 1L) x
f_if_true <- function(x = if (TRUE) 1L else x) x
f_and <- function(x = FALSE && x) x
f_or <- function(x = TRUE || x) x
f_block <- function(x = { if (FALSE) x; 1L }) x
f_nested <- function(x = if (TRUE) { FALSE && x } else x) x
stopifnot(identical(f_if_false(), 1L), identical(f_if_true(), 1L),
          identical(f_and(), FALSE), identical(f_or(), TRUE),
          identical(f_block(), 1L), identical(f_nested(), FALSE))
