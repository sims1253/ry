# no-diag
# IIFE with arguments: (function(v) v * 2L)(5L) returns integer<1>.
# Using the result arithmetically is well-typed.
x <- (function(v) v * 2L)(5L)
y <- x + 1L
