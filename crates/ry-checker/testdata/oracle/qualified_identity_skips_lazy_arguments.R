# oracle: must-pass
quoted <- function(x = x) base::quote(x)
identity <- function(x) NULL
masked <- function(x = x) identity(x)
short <- function(x = x) base::identity(FALSE && x)
branch <- function(x = x) base::identity(if (FALSE) x else 1L)
conditional <- function(flag, x = x) base::identity(if (flag) x else 1L)
stopifnot(identical(quoted(), quote(x)), is.null(masked()),
          identical(short(), FALSE), identical(branch(), 1L),
          identical(conditional(FALSE), 1L))
