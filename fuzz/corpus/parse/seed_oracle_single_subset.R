# oracle: must-pass
# `[` on a vector returns a sub-vector; R succeeds.
x <- c(10, 20, 30)
v <- x[c(1, 3)]
print(v)
