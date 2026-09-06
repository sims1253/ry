# no-diag
quoted <- function(x = x) base::quote(x)
identity <- function(x) NULL
masked <- function(x = x) identity(x)
short <- function(x = x) base::identity(FALSE && x)
branch <- function(x = x) base::identity(if (FALSE) x else 1L)
conditional <- function(flag, x = x) base::identity(if (flag) x else 1L)
