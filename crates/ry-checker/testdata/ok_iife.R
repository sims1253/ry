# no-diag
# Immediately-invoked function expression (IIFE): the function literal
# is called directly. The return type is inferred from the body.
x <- (function() 1L)()
y <- x + 1L
