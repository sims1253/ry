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
