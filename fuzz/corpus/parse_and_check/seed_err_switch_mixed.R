# expect: RY040
# switch() with all-invalid alternatives: a list and a function join to
# union[list, function]. Using the result arithmetically fires RY040
# because EVERY member of the union errors against `+ 1` (an op on a
# union errors only when ALL members error).
x <- switch("a", a = list(1), b = function() { 1 })
bad <- x + 1
