# oracle: must-pass
# Recycling a shorter vector in arithmetic: c(1,2,3) + c(1,2) recycles
# with a warning but succeeds (R produces a result).
x <- c(1, 2, 3) + c(1, 2)
print(x)
