# no-diag
# Anonymous closure passed to a higher-order built-in. v1 does NOT
# infer the callback's return type (that would require modeling how
# `lapply` invokes the callback), so `result` is opaque. The fixture
# is `# no-diag` because there is nothing to flag: `lapply` is in the
# typeshed (resolves to opaque/list), and the anonymous function
# literal is walked for diagnostics without triggering any.
result <- lapply(1:3, function(i) { i * 2 })
