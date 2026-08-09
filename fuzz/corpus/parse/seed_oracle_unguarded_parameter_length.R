# oracle: known-gap a sibling scalar guard does not protect the numeric branch
as_week_start_shape <- function(x) {
  if (is.numeric(x)) {
    if (x > 7 || x < 1) stop("out of range")
  } else {
    if (length(x) != 1) stop("must be scalar")
  }
  x
}
as_week_start_shape(c(1, 2))
