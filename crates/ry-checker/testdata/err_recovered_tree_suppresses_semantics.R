# expect: RY000
# Broken syntax with a downstream semantic finding: the incomplete
# `function(` header makes tree-sitter invent a call expression over the
# remainder of the file, which used to fire RY010 (`variable `x` is not
# bound`) on top of the parse error. Semantic diagnostics derived from a
# recovered tree are suppressed (issue #380): exactly the RY000 remains.
x <- function(
