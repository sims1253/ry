# oracle: must-flag-only RY000
# Byte-for-byte copy of testthat's
# tests/testthat/test-parallel/syntax-error/tests/testthat/test-error-1.R
# at 9b6f12b (the posit corpus pin), prepended with these marker lines
# only. tree-sitter recovers the prose tail as plain identifier
# statements with a zero-width error at EOF, and `Rscript --vanilla`
# fails before reaching it ("could not find function \"test_that\"").
# This is the file whose RY010 false positives on a recovered tree
# motivated #380 and #470's region-aware investigation; whole-file
# suppression (#467) must leave RY000 as the only diagnostic.
test_that("this is good", {
  expect_equal(2 * 2, 4)
})

but this is a syntax error!
