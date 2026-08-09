# no-diag
# A user helper whose formal is captured with substitute() does not force the
# recursive default passed to it.
capture <- function(z) substitute(z)
f <- function(x = x) capture(x)
f()
