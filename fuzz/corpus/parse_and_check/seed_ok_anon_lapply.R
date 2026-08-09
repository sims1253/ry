# no-diag
# Anonymous closure passed to `lapply`. The checker now models the
# callback: `function(i) { i * 2 }` takes an integer (element of
# `1:3`) and returns `i * 2` (integer). The result is a list of
# length 3 with integer elements. No diagnostics: well-typed code.
result <- lapply(1:3, function(i) { i * 2 })
