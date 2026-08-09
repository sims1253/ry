# no-diag
# testthat bodies are braced expressions passed as call arguments; local
# assignments inside the block must be visible to later expectations.
test_that("local variables in a test block resolve", {
  result <- data.frame(x = 1L)
  expect_equal(result$x, 1L)
})
