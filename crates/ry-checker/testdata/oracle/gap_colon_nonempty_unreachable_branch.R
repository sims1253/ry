# oracle: known-gap a branch made unreachable by a nonempty length is still checked, as for literal `1:3`
# `1:n` always has at least one element, so `length(x)` is truthy and the
# else arm never runs. ry still checks that arm and reports RY040; it does
# the same for `x <- 1:3` on main, so this is existing reachability
# behaviour rather than a `:` typing error (#648).
f <- function(n) {
  x <- 1:n
  if (length(x)) x else x + "a"
}
stopifnot(identical(f(3), 1:3))
