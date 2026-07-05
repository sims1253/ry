# oracle: must-pass
# Integer + double coerces to double; R succeeds.
x <- 1L + 2.5
print(x)
