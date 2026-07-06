# no-diag
# A union means checker uncertainty; RY033 should require a proven
# character-vs-non-character comparison.
x <- if (TRUE) 1L else "a"
ok <- x == "a"
