# oracle: must-pass
# R evaluates the comparison element-wise before length(); grep() returns
# positive integer positions by default, so the guard is scalar and exact.
has_match <- function(pattern, x) is.character(x) && length(grep(pattern, x) > 0)
stopifnot(identical(has_match("a", c("a", "a")), TRUE))
stopifnot(identical(has_match("z", c("a", "a")), FALSE))
qualified_match <- function(pattern, x) is.character(x) &&
  length(base::grep(x = x, pattern = pattern) > 0)
stopifnot(identical(qualified_match("a", c("a", "b")), TRUE))
stopifnot(identical(qualified_match("z", c("a", "b")), FALSE))
