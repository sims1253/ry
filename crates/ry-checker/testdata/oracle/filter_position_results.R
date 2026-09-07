# oracle: must-pass
stopifnot(identical(Filter(function(x) FALSE, 1L), integer()))
if (length(Filter(function(x) FALSE, 1L)) == 0) TRUE
stopifnot(identical(Filter(function(x) c(TRUE, FALSE, TRUE), 1:2), c(1L, NA_integer_, NA_integer_, NA_integer_)))
found <- Position(function(x) FALSE, 1:3, nomatch = list(value = NA_real_))
stopifnot(is.na(found$value))
stopifnot(identical(Position(function(x) FALSE, 1:3), NA_integer_))
stopifnot(identical(Position(function(x) x + 1L > 0L, list('a', 1L), right = TRUE), 2L))
`[.filter_result` <- function(x, i, ...) 1:3
x <- structure('a', class = 'filter_result')
result <- Filter(function(x) TRUE, x)
stopifnot(identical(result + 1L, 2:4))
