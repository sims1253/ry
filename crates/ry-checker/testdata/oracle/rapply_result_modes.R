# oracle: must-pass
# Recursive apply matches positional/partial how and can flatten to NULL or vectors.
for (value in list(
  rapply(list(a = 1L), function(x) 1L, "ANY", NULL, "replace"),
  rapply(list(a = 1L), function(x) 1L, ho = "replace"),
  rapply(list(a = 1L), function(x) 1L, how = "rep")
)) stopifnot(identical(value$a, 1L))

empty <- rapply(list(1L), function(x) NULL)
if (length(empty) == 0L) stopifnot(is.null(empty))
expanded <- rapply(list(list(1L, 2L)), function(x) c(x, x))
stopifnot(identical(expanded, c(1L, 1L, 2L, 2L)))
check_callback <- function(callback) {
  result <- rapply(list(1L), callback)
  if (length(result) == 2L) stopifnot(identical(result, c(1L, 2L)))
  result
}
stopifnot(identical(check_callback(function(x) c(1L, 2L)), c(1L, 2L)))
check_mode <- function(how) {
  result <- rapply(list(1L), function(x) c(1L, 2L), how = how)
  if (length(result) == 2L) stopifnot(identical(result, c(1L, 2L)))
  result
}
stopifnot(identical(check_mode("unlist"), c(1L, 2L)))
stopifnot(identical(check_mode("list"), list(c(1L, 2L))))
stopifnot(identical(rapply(expression(1), identity, how = "replace"), expression(1)))

stopifnot(identical(rapply(list(1L), identity, how = "u"), 1L))
for (how in c("l", "r")) {
  stopifnot(identical(rapply(list(1L, 2L), function(x) c(x, x), how = how),
    list(c(1L, 1L), c(2L, 2L))))
}

`+.widget` <- function(e1, e2) 7L
classed <- structure(list(a = 1L), class = "widget")
replaced <- rapply(classed, identity, how = "replace")
stopifnot(inherits(replaced, "widget"), identical(replaced + 1L, 7L))
stopifnot(!inherits(rapply(classed, identity, how = "list"), "widget"))
