# expect: RY040
# If-expression whose branches join to a union of incompatible modes:
# `if (TRUE) list(1) else function() 1` joins to union[list, function].
# Using the result arithmetically fires RY040 because EVERY member of
# the union errors against `+ 1` (Phase 3 union semantics). The earlier
# form of this fixture (`1L else "hello"`) relied on the coercion-ladder
# join that silently promoted to character; that behavior is gone, so
# the fixture now uses an all-invalid union to keep exercising RY040.
x <- if (TRUE) list(1) else function() { 1 }
bad <- x + 1
