# expect: RY001 RY001 RY001 RY001 RY001 RY001 RY001 RY001 RY001 RY001
# Conditions R provably rejects (#373): string literals outside the
# eight accepted spellings (including the "NA" string, which the
# if/while coercion path rejects like any other non-literal text),
# multi-element character, zero-length character, NULL, and list
# values. These keep RY001 while coercible scalar conditions go silent.
a <- if ("x") 1 else 2
b <- if ("tRue") 1 else 2
c <- if ("1") 1 else 2
d <- if ("NA") 1 else 2
e <- if ("") 1 else 2
f <- if (c("TRUE", "FALSE")) 1 else 2
g <- if (character(0)) 1 else 2
h <- if (list(TRUE)) 1 else 2
i <- if (vector("complex", 0)) 1 else 2
j <- if (vector("complex", 2)) 1 else 2
