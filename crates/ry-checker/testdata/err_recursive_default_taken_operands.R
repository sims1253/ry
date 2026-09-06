# expect: RY098, RY098, RY098, RY098, RY098, RY098
# Adjacent taken branches still force the recursive promise.
f_if_true <- function(x = if (TRUE) x else 1L) x
f_if_false <- function(x = if (FALSE) 1L else x) x
f_and <- function(x = TRUE && x) x
f_or <- function(x = FALSE || x) x
f_block <- function(x = { if (TRUE) x; 1L }) x
f_condition <- function(x = if (x) 1L else 2L) x
