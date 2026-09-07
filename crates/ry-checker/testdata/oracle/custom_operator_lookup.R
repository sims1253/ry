# oracle: must-pass
`+` <- function(...) "ok"
result <- "a" + missing_name
stopifnot(identical(result, "ok"))
x <- 1L
ignored <- (x <- "not evaluated") + stop("not evaluated")
stopifnot(identical(x, 1L))
`/` <- function(e1, e2) 1L
stopifnot(identical(1L / 2L, 1L))
`==` <- function(...) 1L
stopifnot(identical(NA == NA, 1L))
`+` <- 7L
stopifnot(identical(1L + 2L, 3L))

`&` <- function(...) "ok"
stopifnot(identical(missing_name & 1L, "ok"))
`|` <- function(...) 1L
stopifnot(identical(missing_name | 1L, 1L))
