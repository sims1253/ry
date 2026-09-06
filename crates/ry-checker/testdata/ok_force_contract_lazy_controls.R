# no-diag
length <- function(x) 1L
masked <- function(x = x) length(x)
unused <- function(x) NULL
ordinary <- function(x = x) unused(x)
quoted <- function(x = x) base::quote(x)
conditional <- function(flag, x = x) base::typeof(if (flag) x else 1L)
short <- function(x = x) base::is.null(FALSE && x)
unrelated <- function(x = x) stats::typeof(x)
