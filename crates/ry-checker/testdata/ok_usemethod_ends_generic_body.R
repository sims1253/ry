# no-diag
f <- function(x, y) {
  UseMethod("f")
  y
}
f.foo <- function(x, y = 1L) y
f(structure(1L, class = "foo"))
