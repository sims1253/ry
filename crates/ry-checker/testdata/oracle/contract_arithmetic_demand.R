# oracle: must-flag-only RY092
# A body that is one base arithmetic operation demands a numeric operand (#646).
f <- function(x) x + 1L
f("oops")
