# oracle: must-warn RY010
# A generic's statements before `UseMethod()` leave its method the
# caller's argument promises (#774), so the method's `{{ variables }}`
# selects `mpg`. A preamble that forces `col` evaluates the bare column
# name first, which errors, and ry reports it.
checked <- function(data, ...) {
  stopifnot(is.data.frame(data))
  UseMethod("checked")
}
checked.data.frame <- function(data, variables, ...) dplyr::select(data, {{ variables }})
stopifnot(identical(names(checked(mtcars, variables = mpg)), "mpg"))
forced <- function(data, col, ...) {
  force(col)
  UseMethod("forced")
}
forced.data.frame <- function(data, col, ...) dplyr::select(data, {{ col }})
stopifnot(inherits(tryCatch(forced(mtcars, col = mpg), error = identity), "error"))
