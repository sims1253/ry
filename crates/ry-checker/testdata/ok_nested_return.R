# no-diag
# Named-return closure: the inner function is bound to `g` inside the
# outer body and then returned as the trailing expression. The body
# simulator processes the `g <- function() { 1L }` assignment so the
# trailing `g` picks up its inferred `fn_sig`. `h()` therefore resolves
# to integer<1>.
f <- function() {
  g <- function() { 1L }
  g
}
h <- f()
v <- h()
