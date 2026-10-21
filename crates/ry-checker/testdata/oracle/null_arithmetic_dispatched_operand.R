# oracle: must-pass
# A literal NULL operand next to a value ry cannot see is not proof of a
# zero-length result: the value's class may provide a `+` method that
# accepts NULL, as ggplot2 layer chains do with a trailing `+ NULL`
# (issue #707).
`+.layers` <- function(e1, e2) {
  if (is.null(e2)) return(e1)
  if (is.null(e1)) return(e2)
  structure(c(unclass(e1), unclass(e2)), class = "layers")
}
add_nothing <- function(plot) plot + NULL
prepend_nothing <- function(plot) NULL + plot
chain <- structure(list("blank"), class = "layers")
stopifnot(
  identical(add_nothing(chain), chain),
  identical(prepend_nothing(chain), chain)
)
