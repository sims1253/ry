# no-diag
# Typed vector constructors: numeric(n), integer(n), character(n),
# logical(n) produce vectors of the right mode with known length.
# Using the results in mode-appropriate operations is well-typed.
x <- numeric(5)
y <- integer(3)
z <- character(10)
a <- logical(1)
b <- complex(2)
c <- double(4)
total <- x[1] + y[1] + a[1] + b[1] + c[1]
