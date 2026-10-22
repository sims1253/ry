# oracle: must-pass
# A call head bound to a value formal falls through to the S4 generic of
# that name: R's function lookup skips the non-function binding.
invisible(setGeneric("area", function(shape) standardGeneric("area")))
invisible(setMethod("area", "numeric", function(shape) shape * 2))
compare <- function(x, area = FALSE) {
  r <- area(x)
  if (area) r else NULL
}
check_all <- function(x) compare(x)
stopifnot(is.null(check_all(1)))
stopifnot(identical(compare(1, TRUE), 2))
