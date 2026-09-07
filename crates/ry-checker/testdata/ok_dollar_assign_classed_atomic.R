# no-diag
`$<-.ry_dollar_widget` <- function(x, name, value) list(saved = value)
x <- structure(1L, class = "ry_dollar_widget")
x$field <- 3L
x$saved + 1L
