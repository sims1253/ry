# no-diag
quoted_default <- function(x = quote(x)) x
substituted_default <- function(x = substitute(x)) x
expression_default <- function(x = expression(x)) x
alist_default <- function(x = alist(x)) x
