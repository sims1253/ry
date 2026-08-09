# no-diag
# A function factory returning a closure. The inner function's return
# type (integer<1>) is captured as a `fn_sig` on the outer function's
# return type, so `c()` resolves to integer<1>. Without closure support
# `c` would be opaque.
make_counter <- function() {
  count <- 0L
  function() {
    count <- count + 1L
    count
  }
}
c <- make_counter()
v <- c()
