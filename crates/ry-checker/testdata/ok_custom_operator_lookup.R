# no-diag
`+` <- function(...) "ok"
result <- "a" + missing_name
x <- 1L
ignored <- (x <- "not evaluated") + stop("not evaluated")
result <- x * 2L
`/` <- function(e1, e2) 1L
integer_result <- 1L / 2L
`==` <- function(...) 1L
comparison_result <- NA == NA
