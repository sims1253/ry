# oracle: must-flag
# An unqualified callback is the project `identity`.
identity <- function(x) "project"
r <- sapply(1:3, identity)
r + 1L
