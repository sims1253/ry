# oracle: must-pass
# Matrix multiplication; R succeeds.
m <- matrix(1:4, 2, 2)
v <- m %*% c(1, 1)
print(v)
