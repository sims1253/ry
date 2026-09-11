# oracle: must-warn RY002
optional <- function(flag) c(if (flag) "a", "b", "c")
stopifnot(length(optional(TRUE)) == 3L, length(optional(FALSE)) == 2L)
flag <- TRUE
values <- if (flag) c(1L, 2L) else c("a", "b")
scalar <- values[1L]
if (scalar != "a") stopifnot(length(scalar) == 1L)
stopifnot(length(c(NULL, 1L, 2L)) == 2L, length(c(list(1L), list(2L))) == 2L)
error <- tryCatch(if (c(TRUE, FALSE)[1:2]) 1L, error = identity)
stopifnot(inherits(error, "error"))
