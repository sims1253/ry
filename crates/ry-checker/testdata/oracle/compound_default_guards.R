# oracle: must-pass
first <- function(x = "default") {
  if (is.character(x) || is.null(x)) 1L else x$field
}
second <- function(x = "default") {
  if (!is.character(x) && !is.null(x)) x$field else 1L
}
stopifnot(first() == 1L, second() == 1L)
stopifnot(first(NULL) == 1L, second(NULL) == 1L)
stopifnot(first(list(field = 2L)) == 2L, second(list(field = 2L)) == 2L)
for (f in list(first, second)) {
  error <- tryCatch(f(1L), error = identity)
  stopifnot(inherits(error, "error"))
  stopifnot(identical(conditionMessage(error), "$ operator is invalid for atomic vectors"))
}
