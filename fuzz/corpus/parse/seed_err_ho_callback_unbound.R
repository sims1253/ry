# expect: RY010
# The callback body is walked for diagnostics: `undefined_var` inside
# the anonymous function passed to `lapply` must trigger RY010.
result <- lapply(1:3, function(i) { undefined_var * 2 })
