# expect: RY091 RY091
# R6 methods, `local()` blocks and testthat blocks each run in their own
# frame. A closure bound there shadows the same-named base function for
# later calls in that frame, as in a plain function body (issue #718).
# Only the two unshadowed base calls at the end are missing arguments.
Mask <- R6::R6Class("Mask",
  public = list(
    run = function(quo) {
      eval <- function() quo
      eval()
    }
  )
)
y <- local({
  nchar <- function() "n"
  nchar()
})
testthat::test_that("a block-local closure shadows base", {
  nchar <- function() "n"
  testthat::expect_equal(nchar(), "n")
})
f <- function(quo) {
  eval <- function() quo
  eval()
}
z <- list(run = function(quo) {
  eval <- function() quo
  eval()
})
v <- local({
  nchar()
})
Unshadowed <- R6::R6Class("Unshadowed",
  public = list(
    run = function() {
      nchar()
    }
  )
)
