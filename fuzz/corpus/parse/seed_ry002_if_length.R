# expect: RY002
# Length-2 logical in `if` would only use the first element with a warning.
if (c(TRUE, FALSE)) print(1)
