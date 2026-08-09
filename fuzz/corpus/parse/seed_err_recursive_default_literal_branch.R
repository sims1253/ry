# expect: RY098
# A literal TRUE branch guarantees that the recursive default is forced.
f <- function(x = x) if (TRUE) x else 1L
f()
