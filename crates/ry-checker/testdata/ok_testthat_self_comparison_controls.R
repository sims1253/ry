# no-diag
# RY122 negative controls: a locally defined (masking) assertion
# function, unknown and effectful same-text callees, differing
# operands, and `$` on a value whose class ry cannot prove plain.
expect_equal <- function(object, expected) object
masked <- function(x) expect_equal(round(x, 3), round(x, 3))
controls <- function(x, stamp) {
  testthat::expect_equal(round(x, 3), round(x, 2))
  testthat::expect_equal(stamp(), stamp())
  testthat::expect_equal(round(x$major, 3), round(x$major, 3))
}
# A rebound unary `-` or a masked `round` makes same-text operands
# effectful, so neither assertion is a proven self-comparison.
neg_control <- function() {
  `-` <- function(e1) rnorm(1)
  testthat::expect_equal(-1, -1)
}
masked_round <- function() {
  round <- function(x, digits = 0) rnorm(1)
  testthat::expect_equal(round(1.5, 3), round(1.5, 3))
}
