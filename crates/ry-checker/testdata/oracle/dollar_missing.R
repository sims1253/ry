# oracle: must-pass
# `$` on a list with a missing name returns NULL (R does not error
# for `$` access to a missing name -- only `[[]]` errors).
v <- list(a = 1)$missing
print(v)
