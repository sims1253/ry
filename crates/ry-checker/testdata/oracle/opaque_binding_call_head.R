# oracle: must-pass
# A call head bound to an opaque value (a parameter or an unknown call
# result) may be a function, and R's call lookup stops at it instead of
# reaching the same-named base or stats function (#722). A local proven
# not to be a function is skipped, so `c(2, 3)` still calls base `c`.
f <- function(dt) dt()$x
g <- function(make) {
  dt <- make()
  d <- dt()
  d$x
}
h <- function(make) {
  nchar <- make()
  nchar()$x
}
u <- function(flag) {
  nchar <- if (flag) NULL else function() list(x = 1)
  nchar()$x
}
make_nchar <- function() function() "n"
k <- function() {
  nchar <- make_nchar()
  nchar()
}
captured <- function(make) {
  nchar <- make()
  inner <- function() nchar()$x
  inner()
}
m <- function() {
  c <- 1
  c(2, 3)
}
make <- function() function() list(x = 1)
stopifnot(identical(f(function() list(x = 1)), 1))
stopifnot(identical(g(make), 1))
stopifnot(identical(h(make), 1))
stopifnot(identical(u(FALSE), 1))
stopifnot(identical(k(), "n"))
stopifnot(identical(captured(make), 1))
stopifnot(identical(m(), c(2, 3)))
