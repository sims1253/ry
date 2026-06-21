# expect: RY040
# `sapply` infers the callback return type (integer from `function(x) x * 2L`)
# and simplifies to an integer vector. Using the result arithmetically
# with a character must fire RY040, proving the type was inferred.
v <- sapply(1:5, function(x) x * 2L)
bad <- v + "hello"
