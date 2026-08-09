# oracle: must-pass
# lapply applies a function over a list; R succeeds.
out <- lapply(list(1, 2, 3), function(x) x * 2)
print(out)
