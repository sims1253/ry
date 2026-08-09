# expect: RY096
# Without `...` in the formals, `hasArg()` on a non-formal can never
# match a supplied argument, so the check is provably always FALSE.
no_dots_shape <- function(x) {
  if (hasArg(threshold)) x <- x + 1
  x
}
