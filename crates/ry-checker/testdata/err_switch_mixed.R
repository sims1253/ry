# expect: RY040
# switch() with all-invalid alternatives: a list and a function join to
# union[list, function]. Using the result arithmetically fires RY040
# because EVERY member of the union errors against `+ 1` (Phase 3 union
# semantics). The earlier form (`1L` and `"two"`) relied on the
# coercion-ladder join that promoted to character; that behavior is
# gone, so the fixture now uses an all-invalid union.
x <- switch("a", a = list(1), b = function() { 1 })
bad <- x + 1
