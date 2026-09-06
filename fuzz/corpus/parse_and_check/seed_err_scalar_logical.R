# expect: RY032
# && with a vector operand: R requires a single value.
x <- c(TRUE, FALSE, TRUE)
bad <- x && TRUE
