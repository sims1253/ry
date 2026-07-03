# oracle: must-pass
# Named arguments in any order; R succeeds.
f <- function(a, b) { a - b }
print(f(b = 10, a = 30))
