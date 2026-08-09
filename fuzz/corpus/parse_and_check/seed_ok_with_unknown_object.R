# no-diag
# with() evaluates names dynamically from its first argument. When that
# object's schema is unknown, bare names inside the expression should
# not be treated as ordinary unbound variables.
render <- function(x) {
  with(x, {
    paste(label, state)
  })
}
