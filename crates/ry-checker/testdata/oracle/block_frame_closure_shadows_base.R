# oracle: must-warn RY091
# `local()` and testthat blocks evaluate their code in a fresh frame. A
# closure bound there shadows the same-named base function for later calls
# in that frame, so each zero-argument call returns the closure's value
# (issue #718). Without the local binding, `nchar()` reaches base and
# errors on its missing `x`; ry keeps that RY091. That check comes first:
# with the search path open, ry cannot prove `local` and `test_that` are
# the base and testthat verbs, so a later call may see `nchar` as a caller
# binding that may be a function, which stays silent (#722).
library(testthat)
unshadowed <- tryCatch(local({
  nchar()
}), error = conditionMessage)
stopifnot(grepl("\"x\" is missing", unshadowed, fixed = TRUE))
shadowed <- local({
  nchar <- function() "n"
  nchar()
})
stopifnot(identical(shadowed, "n"))
test_that("a block-local closure shadows base", {
  nchar <- function() "n"
  stopifnot(identical(nchar(), "n"))
})
