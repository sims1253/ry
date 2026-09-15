# no-diag
# A default may reference a different formal; only self-reference flags.
cross_formal <- function(x = y, y = 1L) x + y
forwarded <- function(by = NULL, copy = by) copy
# Quoted defaults capture the formal without forcing it.
captured <- function(x = quote(x)) class(x)[1L]
substituted <- function(x = substitute(x)) as.character(x)
# A dead self-referential branch of a default is never evaluated.
dead_branch <- function(x = if (FALSE) x else 3L) x
dead_branch()
# Mutual recursion between defaults errors in R when every member of the
# cycle is missing, but detecting it needs a formal-reference cycle analysis;
# RY109 covers only direct self-reference today.
mutual <- function(x = y, y = x) x
mutual(x = 1L)
