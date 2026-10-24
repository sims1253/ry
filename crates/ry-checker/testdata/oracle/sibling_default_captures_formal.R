# oracle: must-warn RY010
# A formal that only a sibling default captures is never forced (#793):
# omitting `q` captures the bare name and supplying `q` never touches it, so
# ry stays silent on both calls. A body that also reads `col` forces it on
# either call, so a bare unbound name errors and ry reports it.
select_enquoted <- function(data, col, q = rlang::enquo(col)) {
  print(q)
  dplyr::select(data, !!q)
}
stopifnot(identical(names(select_enquoted(mtcars, mpg)), "mpg"))
stopifnot(identical(names(select_enquoted(mtcars, not_a_column, q = rlang::quo(mpg))), "mpg"))

select_enquoted_forced <- function(data, col, q = rlang::enquo(col)) {
  print(q)
  print(col)
  dplyr::select(data, !!q)
}
stopifnot(inherits(tryCatch(select_enquoted_forced(mtcars, not_a_column), error = identity), "error"))
stopifnot(inherits(
  tryCatch(select_enquoted_forced(mtcars, not_a_column, q = rlang::quo(mpg)), error = identity),
  "error"
))
