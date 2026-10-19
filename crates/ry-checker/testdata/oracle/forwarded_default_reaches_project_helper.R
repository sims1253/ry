# oracle: must-flag
# An unqualified forward does reach the project `identity` with "text".
identity <- function(x = 0L) x + 1L
wrapper <- function(value = "text") identity(value)
stopifnot(identical(identity(1L), 2L))
wrapper()
