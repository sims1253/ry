# no-diag
# A closure capturing an enclosing-scope binding. `make_adder(x)`
# returns a function whose body references the captured `x`; the inner
# function's return type (double<1>, from `x + y` with both params
# defaulting to double) is recorded as the outer function's `fn_sig`.
# `add5(3)` therefore resolves to double<1>.
make_adder <- function(x = 0) {
  function(y = 0) {
    x + y
  }
}
add5 <- make_adder(5)
v <- add5(3)
