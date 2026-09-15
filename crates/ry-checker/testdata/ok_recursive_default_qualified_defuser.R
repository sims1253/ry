# no-diag
# Qualified base/rlang defusers retain known provenance even when a local
# function shadows the same bare helper name, so this runs and neither
# self-referential default rule fires.
expression <- function(x) x
f <- function(x = x) base::expression(x)
f()
