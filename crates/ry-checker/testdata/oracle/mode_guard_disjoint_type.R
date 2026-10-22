# oracle: must-pass
# A positive mode predicate on a value ry typed with no member of the tested
# mode guards an arm R never enters for those values, so code there that
# would fail on ry's type cannot fail (#752).
pick_list <- function(p) {
  x <- 2L
  if (p) x <- "a"
  if (is.list(x)) x$a
}
stopifnot(is.null(pick_list(TRUE)), is.null(pick_list(FALSE)))

pick_list_else <- function(p) {
  x <- 2L
  if (p) x <- "a"
  if (!is.list(x)) NULL else x$a
}
stopifnot(is.null(pick_list_else(TRUE)))

add_numeric <- function(p) {
  x <- "a"
  if (p) x <- list(1)
  if (is.numeric(x)) x + 1
}
stopifnot(is.null(add_numeric(TRUE)), is.null(add_numeric(FALSE)))
