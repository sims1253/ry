# `missing(p)` asks whether an argument was supplied without forcing its
# promise, so it must not decide whether `p` is defused (#688).

## case: a missing() check before an embrace keeps the parameter defused
# no-diag
select_missing <- function(data, col) {
  if (missing(col)) stop("`col` is required")
  dplyr::select(data, {{ col }})
}
select_missing(mtcars, cyl)

## case: a missing() check before enquo keeps the parameter defused
# no-diag
filter_missing <- function(data, filter) {
  if (missing(filter)) stop("`filter` is required")
  filter <- rlang::enquo(filter)
  dplyr::filter(data, !!filter)
}
filter_missing(mtcars, cyl > 4)

## case: forcing the parameter before the embrace still evaluates it
# expect: RY010
select_forced <- function(data, col) {
  print(col)
  dplyr::select(data, {{ col }})
}
select_forced(mtcars, hp)
