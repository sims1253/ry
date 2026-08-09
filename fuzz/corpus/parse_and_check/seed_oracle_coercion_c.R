# oracle: must-pass
# c() coerces integer + double + character to character; R succeeds.
x <- c(1L, 2.5, "a")
print(x)
