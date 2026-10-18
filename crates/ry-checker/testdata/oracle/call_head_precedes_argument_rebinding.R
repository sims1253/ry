# oracle: must-pass
f <- function(x) { x; "selected" }
value <- f({ f <- function(x, y) { x; y; 1L }; 1L })
stopifnot(identical(value, "selected"))
stopifnot(identical(f(1L, 2L), 1L))
