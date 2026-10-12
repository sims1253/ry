# no-diag
# Without a box call the scalar assertion still holds.
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  length(x)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
