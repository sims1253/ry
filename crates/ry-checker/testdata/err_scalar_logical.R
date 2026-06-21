# expect: RY032
# && with a vector operand: only the first element is used.
x <- c(TRUE, FALSE, TRUE)
bad <- x && TRUE
