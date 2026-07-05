# no-diag
# union[integer, character] + 1 stays quiet: the integer member succeeds,
# the character member errors, but union semantics only flag an
# arithmetic error when ALL members error. R would also produce a result
# here on the integer path (and error at runtime only on the character
# path), so staying quiet is the honest v1 behavior.
p <- TRUE
x <- if (p) 1L else "a"
y <- x + 1
