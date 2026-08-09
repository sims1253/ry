# no-diag
# Typeshed function as callback. `sqrt` is in the typeshed and returns
# double. `sapply(c(1.0, 4.0, 9.0), sqrt)` simplifies to a double
# vector. Using the result arithmetically is well-typed.
v <- sapply(c(1.0, 4.0, 9.0), sqrt)
y <- v + 0.5
