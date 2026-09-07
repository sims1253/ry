# oracle: must-warn RY093
# value = TRUE changes grep() from integer positions to character matches;
# the comparison is therefore outside the proven positional contract.
check <- function(value) {
  length(grep("a", c("a", "b"), value = value) > 0)
}
stopifnot(identical(check(TRUE), 1L), identical(check(FALSE), 1L))
