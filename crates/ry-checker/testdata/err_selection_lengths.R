# expect: RY001, RY040
# Selections re-count their input's elements, but an empty input still
# empties the result, and the result keeps the input's mode.
e <- character(0)
if (head(e, 1) == "a") print("never")
y <- head(letters, 2) + 1
