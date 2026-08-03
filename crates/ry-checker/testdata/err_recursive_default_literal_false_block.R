# expect: RY098
# A literal FALSE branch guarantees that the recursive default is forced
# through the else block.
f <- function(x = x) if (FALSE) 1L else { x }
f()
