# oracle: must-pass
# oracle-claim: RY105
# S3 methods can change the result length even when storage is scalar.
local({
  length.ry_cleanup <- function(x) 0L
  x <- structure(1L, class = "ry_cleanup")
  stopifnot(identical(length(x), 0L))
  if (length(x) > 0L) stop("unreachable")

  class_name <- "ry_cleanup"
  uncertain <- structure(1L, class = class_name)
  stopifnot(identical(length(uncertain), 0L))

  sum.ry_cleanup <- function(x, ...) integer(0)
  stopifnot(identical(length(sum(x)), 0L))
  if (length(sum(x)) > 0L) stop("unreachable")
  if (length(sum(structure(1L, class = "ry_cleanup"))) > 0L) {
    stop("unreachable")
  }

  guarded <- function(x = 1L) {
    if (length(sum(x)) > 0L) stop("unreachable")
  }
  guarded(structure(1L, class = "ry_cleanup"))

  `+` <- function(x, y) structure(1L, class = "ry_cleanup")
  stopifnot(identical(length(sum(1L + 1L)), 0L))
  if (length(sum(1L + 1L)) > 0L) stop("unreachable")

  masked <- function(x) {
    length <- function(x) 1L
    length(x)
  }
  stopifnot(identical(masked(1L), 1L))

  plain <- 1L
  stopifnot(isTRUE(length(plain) > 0L))
})
