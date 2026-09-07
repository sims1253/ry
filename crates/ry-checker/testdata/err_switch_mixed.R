# expect: RY040
# Literal selection returns the list alternative. Arithmetic on that
# selected list errors; the unselected function does not join its type.
x <- switch("a", a = list(1), b = function() { 1 })
bad <- x + 1
