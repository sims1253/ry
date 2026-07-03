# oracle: must-pass
# `[[` on a list returns the element; R succeeds.
lst <- list(a = 1, b = 2)
v <- lst[["a"]]
print(v)
