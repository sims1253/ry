# oracle: must-pass
`n1` <- 42
f <- function() n1 + 1
stopifnot(f() == 43)
`g` <- function(x) x + 1
h <- function() { z <- g; z(1) }
stopifnot(h() == 2)
`value` <- 1
value <- 2
read_value <- function() value
stopifnot(read_value() == 2)
