# oracle: must-pass
# Function with default argument; R succeeds.
f <- function(x, y = 10) { x + y }
print(f(5))
print(f(5, 20))
