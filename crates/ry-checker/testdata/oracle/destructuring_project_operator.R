# oracle: must-pass
# A project-defined `%<-%` (rlang's standalone-zeallot.R, ggdist's
# R/util.R) assigns each name of its `c(...)` pattern in the caller's frame,
# like zeallot's (issue #706). ry binds the pattern names to unknown, so
# `found` no longer keeps its `NULL` placeholder type, and stays silent.
`%<-%` <- function(lhs, value) {
  lhs <- substitute(lhs)
  env <- parent.frame()
  vars <- as.list(lhs[-1])
  for (i in seq_along(vars)) assign(as.character(vars[[i]]), value[[i]], envir = env)
  invisible(value)
}
f <- function() {
  found <- NULL
  c(found, other) %<-% list(TRUE, 2)
  if (found) other
}
stopifnot(identical(f(), 2))
