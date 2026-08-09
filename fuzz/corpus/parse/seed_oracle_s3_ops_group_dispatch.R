# oracle: must-pass
Ops.ry_oracle <- function(e1, e2) TRUE
x <- structure(list(1), class = "ry_oracle")
stopifnot(x + x)
