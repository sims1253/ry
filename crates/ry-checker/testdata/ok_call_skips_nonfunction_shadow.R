# no-diag
# R's function/value namespace separation: a CALL searches for a
# function named `f` and skips non-function bindings. A local
# non-function binding with the same name as a typeshed function must
# NOT fire RY070 at the call site. (Exercises the `lengths` shadowing
# pattern the glue vendor net caught.)
f <- function(unnamed_args) {
  lengths <- lengths(unnamed_args)
  res <- lengths(list(1:3))
  if (any(lengths == 0)) return(NULL)
  res
}
