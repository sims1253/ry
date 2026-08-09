# expect: RY001
# Triggered by `if` on a non-logical atomic (character coerces to NA,
# which is almost always a bug).
if ("x") print(1)
