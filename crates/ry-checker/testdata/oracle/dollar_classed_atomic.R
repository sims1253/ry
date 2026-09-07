# oracle: must-pass
# An atomic payload can dispatch `$` to an S3 method with an arbitrary result.
`$.ry_dollar_widget` <- function(x, name) list(value = 2L)
x <- structure(1L, class = "ry_dollar_widget")
result <- x$field
stopifnot(identical(result$value, 2L))
