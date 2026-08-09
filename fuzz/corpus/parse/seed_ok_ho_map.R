# no-diag
# `Map` with a binary callback. `function(a, b) a + b` takes two
# doubles and returns double. `Map(f, c(1, 2), c(3, 4))` returns a
# list of length 2. No diagnostics: the callback body is well-typed.
result <- Map(function(a, b) a + b, c(1.0, 2.0), c(3.0, 4.0))
