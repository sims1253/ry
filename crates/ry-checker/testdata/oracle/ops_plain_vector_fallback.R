# oracle: must-warn RY051
# Explicit FALSE/FALSE choices fall back to vector primitives.
x <- base::structure(base::c(1L, 2L), class = 'left')
y <- base::structure(base::c(3L, 4L, 5L), class = 'right')
`+.left` <- function(e1, e2) 1L
`+.right` <- function(e1, e2) 'method'
chooseOpsMethod.left <- function(...) FALSE
chooseOpsMethod.right <- function(...) FALSE
z <- x + y
stopifnot(typeof(z) == 'integer', length(z) == 3L,
          identical(class(z), 'right'), identical(unclass(z), c(4L, 6L, 6L)))
# c() is NULL, not an empty atomic payload; adding attributes errors in R.
stopifnot(is.null(base::c()),
          inherits(try(base::structure(base::c(), class = 'left'), silent = TRUE), 'try-error'))
# R keeps the left class on equal lengths and drops classes for comparison/logical.
y_equal <- structure(c(3L, 4L), class = 'right')
stopifnot(identical(class(suppressWarnings(x + y_equal)), 'left'))
`==.left` <- function(e1,e2) 1L
`==.right` <- function(e1,e2) 'method'
`&.left` <- function(e1,e2) 1L
`&.right` <- function(e1,e2) 'method'
stopifnot(is.null(attributes(suppressWarnings(x == y_equal))),
          is.null(attributes(suppressWarnings(x & y_equal))))
# Equal lengths do not prove array compatibility; dimensions remain excluded.
a <- structure(c(1L,2L,3L,4L), dim=c(2L,2L), class='left')
b <- structure(c(1L,2L,3L,4L), dim=c(4L,1L), class='right')
stopifnot(inherits(try(suppressWarnings(a+b), silent=TRUE), 'try-error'))
