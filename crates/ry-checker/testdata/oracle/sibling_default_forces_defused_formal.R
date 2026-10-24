# oracle: must-warn RY010
# Using `label`, whose default reads `col`, forces `col` before the
# embrace (#790), so a bare unbound name errors when the call omits
# `label`, and ry reports it. A call that supplies `label` never runs the
# default and selects the column.
select_labelled <- function(data, col, label = col) {
  print(label)
  dplyr::select(data, {{ col }})
}
stopifnot(inherits(tryCatch(select_labelled(mtcars, not_a_column), error = identity), "error"))
stopifnot(identical(names(select_labelled(mtcars, mpg, label = "miles")), "mpg"))
