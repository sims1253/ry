# expect: RY122, RY122, RY122
# ggplot2 test-axis-secondary shape: both operands are the same
# deterministic expression, so the assertion cannot check primary
# against secondary breaks. The differing-operand and effectful
# same-text lines are the adjacent idioms that must stay quiet.
reduced <- function(x) testthat::expect_equal(round(x, 3), round(x, 3))
secondary_axis <- function() {
  breaks <- list(major = 1.23456, sec.major = 9.87654)
  testthat::expect_equal(round(breaks$major, 3), round(breaks$major, 3))
  testthat::expect_identical(breaks$major, breaks$major)
  testthat::expect_equal(round(breaks$major, 3), round(breaks$sec.major, 3))
  testthat::expect_equal(rnorm(1), rnorm(1))
  testthat::expect_equal(Sys.time(), Sys.time())
}
