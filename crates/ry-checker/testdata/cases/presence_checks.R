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

## case: a missing()-only validator before an embrace keeps the parameter defused
# no-diag
check_not_missing <- function(x) {
  if (missing(x)) stop("argument is missing")
  invisible()
}
select_checked <- function(data, col) {
  check_not_missing(col)
  dplyr::select(data, {{ col }})
}
select_checked(mtcars, mpg)

## case: a missing()-only validator before enquo keeps the parameter defused
# no-diag
check_not_missing <- function(x) if (missing(x)) stop("argument is missing")
filter_checked <- function(data, filter) {
  check_not_missing(filter)
  filter <- rlang::enquo(filter)
  dplyr::filter(data, !!filter)
}
filter_checked(mtcars, cyl > 4)

## case: a validator that also reads its formal still forces the parameter
# expect: RY010
check_and_print <- function(x) {
  if (missing(x)) stop("argument is missing")
  print(x)
}
select_printed <- function(data, col) {
  check_and_print(col)
  dplyr::select(data, {{ col }})
}
select_printed(mtcars, wt)

## case: an unresolved validator still forces the parameter
# expect: RY010
select_validated <- function(data, col) {
  validate_column(col)
  dplyr::select(data, {{ col }})
}
select_validated(mtcars, qsec)
