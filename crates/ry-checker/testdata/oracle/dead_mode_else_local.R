# oracle: must-pass
# A local whose type a mode test accepts never reaches that test's rejecting
# arm, so code there that would fail on the local's type cannot fail (#710).
is_tensor <- function(x) inherits(x, "tensor")
reshape_dead_else <- function(x, shape = NULL) {
  if (!is.null(shape) && !is_tensor(shape)) shape <- as.integer(shape)
  if (!is.null(shape)) {
    if (is.numeric(shape)) shape <- as.list(shape)
    else if (is_tensor(shape) && shape$shape$rank == 0) shape <- 1
  }
  x
}
dead_not_numeric <- function() {
  s <- 1L
  if (!is.numeric(s)) s$x
}
stopifnot(identical(reshape_dead_else(1), 1), identical(reshape_dead_else(1, 2), 1))
stopifnot(is.null(dead_not_numeric()))
