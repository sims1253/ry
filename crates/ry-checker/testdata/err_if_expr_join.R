# expect: RY040
# If-expression with incompatible branches: `if (TRUE) 1L else "hello"`
# joins to character. Using the result arithmetically fires RY040.
x <- if (TRUE) 1L else "hello"
bad <- x + 1
