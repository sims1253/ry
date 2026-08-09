# oracle: must-pass
# switch with no match and no default returns NULL; R succeeds.
v <- switch("z", a = 1, b = 2)
print(v)
