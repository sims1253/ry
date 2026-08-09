# oracle: must-pass
# switch with a matching alternative; R succeeds.
v <- switch("b", a = 1, b = 2, c = 3)
print(v)
