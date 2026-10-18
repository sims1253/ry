# oracle: must-pass
is_null <- function() function(x) x
is_false <- function() function(x) x
check <- function(x, allow_null) {
  if (allow_null && rlang::is_null(x)) TRUE else FALSE
}
stopifnot(check(NULL, TRUE))
if (rlang::is_false(FALSE)) 1L
