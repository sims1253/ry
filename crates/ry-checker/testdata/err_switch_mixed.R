# expect: RY040
# switch() with mixed-type alternatives: 1L and "two" join to character.
# Using the result arithmetically fires RY040.
x <- switch("b", a = 1L, b = "two")
bad <- x + 1
