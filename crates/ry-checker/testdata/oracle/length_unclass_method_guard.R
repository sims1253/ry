# oracle: must-pass
# A `length.<class>` method whose body is `length(unclass(x))` reports the
# storage length, so `length(x) == 1L` still proves the operand scalar and
# the guarded `is.na(x)` never sees a vector (#701).
length.cell <- function(x) length(unclass(x))
is_unset <- function(x) is.null(x) || (length(x) == 1L && is.na(x))
stopifnot(
  identical(length(structure(c(NA, 1), class = "cell")), 2L),
  isFALSE(is_unset(structure(c(NA, 1), class = "cell"))),
  isTRUE(is_unset(structure(NA, class = "cell"))),
  isFALSE(is_unset(c(NA, NA)))
)
