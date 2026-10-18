# oracle: must-pass
identity <- function(x) "project"
value <- base::identity(1L)
stopifnot(identical(value, 1L))
value + 1L
