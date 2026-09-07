# oracle: must-pass
# Every reachable branch replaces the initial binding before it is used.
choose_callback <- function(flag, nested) {
  callback <- NULL
  if (flag) {
    if (nested) callback <- function(x) x else callback <- function(x) x + 1L
  } else callback <- function(x) x + 2L
  callback(1L)
}
stopifnot(identical(choose_callback(TRUE, TRUE), 1L))
stopifnot(identical(choose_callback(TRUE, FALSE), 2L))
stopifnot(identical(choose_callback(FALSE, TRUE), 3L))
stopifnot(identical(choose_callback(FALSE, FALSE), 3L))

replace_integer <- function(flag) {
  value <- 1L
  if (flag) value <- "first" else value <- "second"
  value
}
stopifnot(identical(replace_integer(TRUE), "first"))
stopifnot(identical(replace_integer(FALSE), "second"))
