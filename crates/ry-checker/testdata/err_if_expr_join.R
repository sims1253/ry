# expect: RY040
# If-expression whose branches join to a union of incompatible modes:
# `if (TRUE) list(1) else function() 1` joins to union[list, function].
# Using the result arithmetically fires RY040 because EVERY member of
# the union errors against `+ 1` (an op on a union errors only when ALL
# members error).
x <- if (TRUE) list(1) else function() { 1 }
bad <- x + 1
